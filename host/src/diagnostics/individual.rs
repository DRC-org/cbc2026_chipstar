//! 個別テストの対象・単位・指令。通信と操作権はapplicationが所有する。
use crate::{
    machine::MachineProfile,
    protocol::{dcmd, serial_svmd, svmd},
};
use anyhow::{Context, Result, bail, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Cctl(u8),
    Pwm(u8),
    Sts(u8),
    Dc,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Position,
    Velocity,
    Duty,
}
impl Kind {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "position" => Ok(Self::Position),
            "velocity" => Ok(Self::Velocity),
            "duty" => Ok(Self::Duty),
            _ => bail!("テスト方式が不正です"),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Velocity => "velocity",
            Self::Duty => "duty",
        }
    }
    pub fn momentary(self) -> bool {
        self != Self::Position
    }
}
impl Target {
    pub fn parse(text: &str) -> Result<Self> {
        let (board, id) = text
            .split_once(':')
            .context("基板:チャネルを指定してください")?;
        let id: u8 = id.parse()?;
        match (board, id) {
            ("cctl", 0..=2) => Ok(Self::Cctl(id)),
            ("pwm", 0..=3) => Ok(Self::Pwm(id)),
            ("sts", 1..=253) => Ok(Self::Sts(id)),
            ("dc", 0) => Ok(Self::Dc),
            _ => bail!("未対応の基板・チャネルです"),
        }
    }
    pub fn key(self) -> String {
        match self {
            Self::Cctl(id) => format!("cctl:{id}"),
            Self::Pwm(id) => format!("pwm:{id}"),
            Self::Sts(id) => format!("sts:{id}"),
            Self::Dc => "dc:0".into(),
        }
    }
    pub fn board(self) -> &'static str {
        match self {
            Self::Cctl(_) => "cctl",
            Self::Pwm(_) => "pwm",
            Self::Sts(_) => "sts",
            Self::Dc => "dc",
        }
    }
    pub fn limits(self, kind: Kind, profile: &MachineProfile) -> Result<(f32, f32, String)> {
        if kind == Kind::Position
            && let Some(axis) = crate::machine::ee::axes(profile)
                .iter()
                .find(|a| a.target == self)
        {
            return Ok((axis.min, axis.max, axis.unit().into()));
        }
        Ok(match (self, kind) {
            (Self::Cctl(slot), Kind::Position) => {
                let axis = profile
                    .axes
                    .iter()
                    .find(|axis| axis.slot == slot)
                    .context("軸設定がありません")?;
                (axis.minimum, axis.maximum, axis.unit.clone())
            }
            (Self::Cctl(slot), Kind::Velocity) => {
                let axis = profile
                    .axes
                    .iter()
                    .find(|a| a.slot == slot)
                    .context("軸設定がありません")?;
                let cap = (axis.speed_per_second
                    * axis.native_per_unit
                    * profile.slow_speed_percent
                    * 0.01)
                    .abs();
                (
                    -cap,
                    cap,
                    if slot == 1 || slot == 2 {
                        "motor deg/s"
                    } else {
                        "rad/s"
                    }
                    .into(),
                )
            }
            (Self::Pwm(_), Kind::Position) => (
                profile
                    .svmd_parameters
                    .get("min_pulse_us")
                    .copied()
                    .unwrap_or(500.0),
                profile
                    .svmd_parameters
                    .get("max_pulse_us")
                    .copied()
                    .unwrap_or(2500.0),
                "µs".into(),
            ),
            (Self::Sts(_), Kind::Position) => (0.0, 4095.0, "step".into()),
            (Self::Dc, Kind::Duty) => (-100.0, 100.0, "‰".into()),
            _ => bail!("この対象では使えないテスト方式です"),
        })
    }
    pub fn validate(self, kind: Kind, value: f32, profile: &MachineProfile) -> Result<()> {
        let (min, max, _) = self.limits(kind, profile)?;
        ensure!(
            value.is_finite() && (min..=max).contains(&value),
            "指令値は{min}..{max}で指定してください"
        );
        if !matches!(self, Self::Cctl(_)) {
            ensure!(value.fract() == 0.0, "整数で指定してください");
        }
        Ok(())
    }
    pub fn begin(self) -> Vec<String> {
        match self {
            Self::Cctl(id) => vec![
                "ENABLE 7 0".into(),
                format!("ENABLE {} 1", 1u8 << id),
                "RUN".into(),
            ],
            Self::Sts(_) => vec![serial_svmd::Command::Safe.to_cctl_line()],
            Self::Dc => vec![dcmd::line(0, 0, 0)],
            Self::Pwm(_) => vec![],
        }
    }
    pub fn command(self, kind: Kind, value: f32) -> Vec<String> {
        match self {
            Self::Cctl(id) => vec![format!(
                "{} {id} {value}",
                if kind == Kind::Position {
                    "TARGET"
                } else {
                    "JOG"
                }
            )],
            Self::Pwm(channel) => vec![
                svmd::Command::Set {
                    channel,
                    pulse_us: value as u16,
                }
                .to_cctl_line(),
                svmd::Command::Enable {
                    channel,
                    enabled: true,
                }
                .to_cctl_line(),
            ],
            Self::Sts(id) => vec![
                serial_svmd::Command::Target {
                    id,
                    position: value as i16,
                    speed: 100,
                    acceleration: 10,
                }
                .to_cctl_line(),
                serial_svmd::Command::Enable { id, enabled: true }.to_cctl_line(),
                serial_svmd::Command::Run.to_cctl_line(),
            ],
            Self::Dc => vec![dcmd::line(4, 0, value as i16), dcmd::line(2, 1, 0)],
        }
    }
    pub fn heartbeat(self) -> &'static str {
        match self {
            Self::Cctl(_) => "HEARTBEAT",
            Self::Pwm(_) => "CAN 2 768 0103000000000000",
            Self::Sts(_) => "CAN 2 800 0105000000000000",
            Self::Dc => "CAN 2 784 0105000000000000",
        }
    }
}
