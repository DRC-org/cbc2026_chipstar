//! プロトコルの模擬応答。実機の衝突や制動距離は再現しない。
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};

pub struct Simulator {
    started: Instant,
    tick: Instant,
    contact: Instant,
    mode: &'static str,
    enabled: u8,
    position: [f32; 3],
    velocity: [f32; 3],
    targets: [Option<f32>; 3],
    pwm: u8,
    servos: BTreeMap<u8, (u16, bool)>,
    servo_mode: u8,
    sts: super::sts_simulator::StsSimulator,
    dc_enabled: bool,
    dc_duty: i16,
    parameters: BTreeMap<u8, f32>,
    rx: VecDeque<String>,
    disconnected: bool,
    reject: bool,
    delay_run: bool,
    pending_run: Option<Instant>,
}
impl Simulator {
    pub(super) fn fault(&mut self, fault: &str) -> Result<()> {
        match fault {
            "disconnect" => {
                self.sts.stop();
                self.disconnected = true;
                self.pwm = 0;
                self.dc_enabled = false;
                self.dc_duty = 0;
                for servo in self.servos.values_mut() {
                    servo.1 = false;
                }
                self.velocity = [0.0; 3];
                self.mode = "STOP";
                self.rx.clear();
            }
            "reconnect" => {
                self.disconnected = false;
                self.mode = "SAFE";
                self.enabled = 0;
                self.pending_run = None;
                self.started = Instant::now();
                self.parameters.clear();
            }
            "reject" => self.reject = true,
            "delay_run" => self.delay_run = true,
            _ => bail!("disconnect / reconnect / reject / delay_run を指定してください"),
        }
        Ok(())
    }

    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            tick: Instant::now(),
            contact: Instant::now(),
            mode: "SAFE",
            enabled: 0,
            position: [0.0; 3],
            velocity: [0.0; 3],
            targets: [None; 3],
            pwm: 0,
            servos: BTreeMap::new(),
            servo_mode: 0,
            sts: super::sts_simulator::StsSimulator::default(),
            dc_enabled: false,
            dc_duty: 0,
            parameters: BTreeMap::new(),
            rx: VecDeque::new(),
            disconnected: false,
            reject: false,
            delay_run: false,
            pending_run: None,
        }
    }
    pub(super) fn write(&mut self, line: &str) -> Result<()> {
        if self.disconnected {
            bail!("模擬通信断");
        }
        let words: Vec<_> = line.split_whitespace().collect();
        if self.reject && matches!(words.first().copied(), Some("JOG" | "PARAM" | "RUN")) {
            self.reject = false;
            self.rx.push_back("ERR code=SIM_REJECTED".into());
            return Ok(());
        }
        match words.as_slice() {
            ["HELLO", "1"] => self.rx.push_back(
                "DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250 params=default jog=1 motors=el05,m3508,m3508"
                    .into(),
            ),
            ["HEARTBEAT"] => self.contact = Instant::now(),
            ["STOP"] | ["SAFE"] => {
                self.mode = if line == "STOP" { "STOP" } else { "SAFE" };
                self.velocity = [0.0; 3];
                self.targets = [None; 3];
                self.pending_run = None;
            }
            ["RUN"] => {
                if self.delay_run {
                    self.delay_run = false;
                    self.pending_run = Some(Instant::now() + std::time::Duration::from_millis(250));
                } else {
                    self.mode = "RUN";
                    self.velocity = [0.0; 3];
                    self.contact = Instant::now();
                }
            }
            ["ENABLE", mask, flag] => {
                let mask: u8 = mask.parse()?;
                if *flag == "1" {
                    self.enabled |= mask;
                } else {
                    self.enabled &= !mask;
                }
            }
            ["JOG", slot, value] => {
                let slot: usize = slot.parse()?;
                let value: f32 = value.parse()?;
                if slot >= 3
                    || self.mode != "RUN"
                    || self.enabled & (1 << slot) == 0
                    || !value.is_finite()
                {
                    self.rx.push_back("ERR code=JOG_REJECTED".into());
                } else {
                    self.targets[slot] = None;
                    self.velocity[slot] = value;
                    self.contact = Instant::now();
                }
            }
            ["TARGET", slot, value] => {
                let slot: usize = slot.parse()?;
                let value: f32 = value.parse()?;
                if slot >= 3 || !value.is_finite() {
                    self.rx.push_back("ERR code=TARGET_REJECTED".into());
                } else {
                    self.targets[slot] = Some(value);
                    self.velocity[slot] = 0.0;
                }
            }
            ["REINIT", _] => {
                self.mode = "SAFE";
                self.targets = [None; 3];
                self.velocity = [0.0; 3];
            }
            ["PARAM", id, value] => {
                let id: u8 = id.parse()?;
                let value: f32 = value.parse()?;
                self.parameters.insert(id, value);
                self.rx.push_back(format!("PARAM {id} {value}"));
            }
            ["PARAM", id] => {
                let id: u8 = id.parse()?;
                self.rx.push_back(format!(
                    "PARAM {id} {}",
                    self.parameters.get(&id).unwrap_or(&0.0)
                ));
            }
            ["CAN", "2", id, payload] => {
                let id: u16 = id.parse()?;
                if payload.len() == 16 {
                    let mut bytes = [0u8; 8];
                    for (i, byte) in bytes.iter_mut().enumerate() {
                        *byte = u8::from_str_radix(&payload[i * 2..i * 2 + 2], 16)?;
                    }
                    if id == 800 && bytes[1] >= 20 {
                        self.sts.handle(bytes, self.servo_mode == 1, &mut self.rx);
                    } else if (id == 768 && bytes[1] == 4)
                        || (id == 784 && bytes[1] == 7)
                        || (id == 800 && bytes[1] == 9)
                    {
                        self.rx.push_back(format!(
                            "CAN_RX bus=2 id={} data=01{:02X}0000{}",
                            id + 4,
                            bytes[2],
                            &payload[8..]
                        ));
                    } else {
                        match id {
                            768 => {
                                if bytes[1] == 0 {
                                    self.pwm = 0;
                                }
                                if bytes[1] == 2 && bytes[2] < 4 {
                                    if bytes[3] == 1 {
                                        self.pwm |= 1 << bytes[2];
                                    } else {
                                        self.pwm &= !(1 << bytes[2]);
                                    }
                                }
                                self.rx.push_back(format!(
                                    "CAN_RX bus=2 id=769 data=0100{:02X}{:02X}{:02X}000000",
                                    bytes[1], bytes[2], self.pwm
                                ));
                            }
                            784 => {
                                match bytes[1] {
                                    2 => self.dc_enabled = bytes[2] == 1,
                                    3 => {
                                        self.dc_enabled = false;
                                        self.dc_duty = 0;
                                    }
                                    4 => self.dc_duty = i16::from_be_bytes([bytes[4], bytes[5]]),
                                    6 => self.rx.push_back(
                                        "CAN_RX bus=2 id=787 data=0100000007000000".into(),
                                    ),
                                    _ => {}
                                }
                                let [hi, lo] = self.dc_duty.to_be_bytes();
                                self.rx.push_back(format!(
                                    "CAN_RX bus=2 id=785 data=0100{:02X}{:02X}{hi:02X}{lo:02X}0000",
                                    u8::from(self.dc_enabled),
                                    u8::from(self.dc_enabled)
                                ));
                                self.rx
                                    .push_back("CAN_RX bus=2 id=786 data=0101000000000000".into());
                            }
                            800 => {
                                match bytes[1] {
                                    1 | 3 => {
                                        self.sts.stop();
                                        self.servo_mode = if bytes[1] == 1 { 0 } else { 2 };
                                        for servo in self.servos.values_mut() {
                                            servo.1 = false;
                                        }
                                    }
                                    2 => self.servo_mode = 1,
                                    4 => {
                                        self.servos.entry(bytes[2]).or_default().0 =
                                            u16::from_be_bytes([bytes[4], bytes[5]]);
                                    }
                                    6 => {
                                        self.servos.entry(bytes[2]).or_default().1 = bytes[3] == 1;
                                    }
                                    7 => {
                                        let (position, enabled) =
                                            self.servos.get(&bytes[2]).copied().unwrap_or_default();
                                        self.rx.push_back(format!("CAN_RX bus=2 id=802 data=01{:02X}{position:04X}{:02X}000000", bytes[2], u8::from(enabled)));
                                    }
                                    8 => self.rx.push_back(
                                        "CAN_RX bus=2 id=803 data=010000003F000000".into(),
                                    ),
                                    _ => {}
                                }
                                self.rx.push_back(format!(
                                    "CAN_RX bus=2 id=801 data=0100{:02X}{:02X}00000000",
                                    self.servo_mode,
                                    self.servos.values().filter(|servo| servo.1).count()
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => self.rx.push_back("ERR code=BAD_COMMAND".into()),
        }
        Ok(())
    }
    pub(super) fn read(&mut self) -> Vec<String> {
        let dt = self.tick.elapsed().as_secs_f32().min(0.1);
        self.tick = Instant::now();
        if self.disconnected {
            return Vec::new();
        }
        if self
            .pending_run
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.pending_run = None;
            self.mode = "RUN";
            self.velocity = [0.0; 3];
            self.contact = Instant::now();
        }
        if self.contact.elapsed().as_millis() > 250 && self.mode == "RUN" {
            self.mode = "STOP";
            self.velocity = [0.0; 3];
        }
        if self.mode == "RUN" {
            let caps = [
                *self.parameters.get(&9).unwrap_or(&1.0),
                self.parameters.get(&3).unwrap_or(&500.0) * 6.0,
                self.parameters.get(&36).unwrap_or(&500.0) * 6.0,
            ];
            for (i, cap) in caps.iter().enumerate() {
                if self.enabled & (1 << i) != 0 {
                    if let Some(target) = self.targets[i] {
                        self.position[i] = target;
                    }
                    self.position[i] += self.velocity[i].clamp(-cap.abs(), cap.abs()) * dt;
                    let minimum = *self.parameters.get(&(15 + 2 * i as u8)).unwrap_or(&-1.0e6);
                    let maximum = *self.parameters.get(&(16 + 2 * i as u8)).unwrap_or(&1.0e6);
                    if minimum < maximum {
                        self.position[i] = self.position[i].clamp(minimum, maximum);
                    }
                }
            }
        }
        let mut lines: Vec<_> = self.rx.drain(..).collect();
        lines.push(format!("STATE t={} mode={} en={} a0={p0}/{p0} a1={p1}/{p1} a2={p2}/{p2} err=00,00,00 sw=7 stale=0 can=3", self.started.elapsed().as_millis(), self.mode, self.enabled, p0=self.position[0], p1=self.position[1], p2=self.position[2]));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn slot2_speed_limit_uses_its_own_rotor_rpm() {
        let mut sim = Simulator::new();
        for line in [
            "PARAM 3 500",
            "PARAM 36 10",
            "ENABLE 6 1",
            "RUN",
            "JOG 1 600",
            "JOG 2 600",
        ] {
            sim.write(line).unwrap();
        }
        sim.tick = Instant::now() - Duration::from_secs(1);
        sim.read();
        assert!((sim.position[1] - 60.0).abs() < 0.001);
        assert!((sim.position[2] - 6.0).abs() < 0.001);
        sim.write("STOP").unwrap();
        let held = sim.position;
        sim.read();
        assert_eq!(sim.position, held);
    }
}
