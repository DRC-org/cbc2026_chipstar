//! 機体設定、原点管理、手動速度指令。
mod controller;
mod profile;
pub use controller::{MachineController, OriginState};
pub use profile::*;
pub mod dc_motor;
pub mod ee;
