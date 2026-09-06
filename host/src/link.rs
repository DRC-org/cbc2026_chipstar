//! 実機と模擬機体は同じ指令・応答経路を通る。
use crate::serial::SerialLink;
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};

pub enum Link {
    Real(SerialLink),
    Sim(Simulator),
}
impl Link {
    pub fn new(device: &str, baud: u32, simulate: bool) -> Self {
        if simulate {
            Self::Sim(Simulator::new())
        } else {
            Self::Real(SerialLink::new(device.into(), baud))
        }
    }
    pub fn write_line(&mut self, line: &str) -> Result<()> {
        match self {
            Self::Real(link) => link.write_line(line),
            Self::Sim(sim) => sim.write(line),
        }
    }
    pub fn read_lines(&mut self) -> Vec<String> {
        match self {
            Self::Real(link) => link.read_lines(),
            Self::Sim(sim) => sim.read(),
        }
    }
    pub fn fault(&mut self, fault: &str) -> Result<()> {
        let Self::Sim(sim) = self else {
            bail!("模擬接続でのみ使用できます");
        };
        match fault {
            "disconnect" => {
                sim.disconnected = true;
                sim.velocity = [0.0; 3];
                sim.mode = "STOP";
                sim.rx.clear();
            }
            "reconnect" => {
                sim.disconnected = false;
                sim.mode = "SAFE";
                sim.enabled = 0;
                sim.started = Instant::now();
                sim.parameters.clear();
            }
            "reject" => sim.reject = true,
            _ => bail!("disconnect / reconnect / reject を指定してください"),
        }
        Ok(())
    }
}

pub struct Simulator {
    started: Instant,
    tick: Instant,
    contact: Instant,
    mode: &'static str,
    enabled: u8,
    position: [f32; 3],
    velocity: [f32; 3],
    parameters: BTreeMap<u8, f32>,
    rx: VecDeque<String>,
    disconnected: bool,
    reject: bool,
}
impl Simulator {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            tick: Instant::now(),
            contact: Instant::now(),
            mode: "SAFE",
            enabled: 0,
            position: [0.0; 3],
            velocity: [0.0; 3],
            parameters: BTreeMap::new(),
            rx: VecDeque::new(),
            disconnected: false,
            reject: false,
        }
    }
    fn write(&mut self, line: &str) -> Result<()> {
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
                "DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250 params=default jog=1"
                    .into(),
            ),
            ["HEARTBEAT"] => self.contact = Instant::now(),
            ["STOP"] | ["SAFE"] => {
                self.mode = if line == "STOP" { "STOP" } else { "SAFE" };
                self.velocity = [0.0; 3];
            }
            ["RUN"] => {
                self.mode = "RUN";
                self.velocity = [0.0; 3];
                self.contact = Instant::now();
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
                    self.velocity[slot] = value;
                    self.contact = Instant::now();
                }
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
                    if (id == 768 && bytes[1] == 4)
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
                        self.rx
                            .push_back(format!("CAN_RX bus=2 id={} data=0100000000000000", id + 1));
                    }
                }
            }
            _ => self.rx.push_back("ERR code=BAD_COMMAND".into()),
        }
        Ok(())
    }
    fn read(&mut self) -> Vec<String> {
        let dt = self.tick.elapsed().as_secs_f32().min(0.1);
        self.tick = Instant::now();
        if self.disconnected {
            return Vec::new();
        }
        if self.contact.elapsed().as_millis() > 250 && self.mode == "RUN" {
            self.mode = "STOP";
            self.velocity = [0.0; 3];
        }
        if self.mode == "RUN" {
            let caps = [
                *self.parameters.get(&9).unwrap_or(&1.0),
                self.parameters.get(&3).unwrap_or(&500.0) * 6.0,
                *self.parameters.get(&14).unwrap_or(&1.0),
            ];
            for (i, cap) in caps.iter().enumerate() {
                if self.enabled & (1 << i) != 0 {
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
