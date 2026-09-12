//! 機体設定、原点管理、手動速度指令。
mod controller;
pub(crate) mod motion_profile;
mod profile;
pub use controller::{MachineController, OriginState};
pub use profile::*;
pub mod bonus;
pub mod dc_motor;
pub mod ee;
pub mod xy;

#[cfg(test)]
pub(crate) mod test_support;
