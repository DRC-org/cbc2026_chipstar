//! 実機と模擬機体は同じ指令・応答経路を通る。
use crate::transport::serial::SerialLink;
use anyhow::{Result, bail};

use super::simulator::Simulator;

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
        sim.fault(fault)
    }
}
