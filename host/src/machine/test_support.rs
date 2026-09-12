use super::{MachineProfile, SerialSvmdProfile, bonus::BonusProfile, dc_motor::MotorProfile};
use serde::Deserialize;

#[derive(Deserialize)]
struct BonusFixture {
    bonus: BonusProfile,
    dc_motors: Vec<MotorProfile>,
    serial_svmd: SerialSvmdProfile,
}

/// ボーナス機能のテストは、実機での搭載有無に依存させない。
pub(crate) fn with_bonus() -> MachineProfile {
    let fixture: BonusFixture =
        toml::from_str(include_str!("../../tests/fixtures/bonus.toml")).unwrap();
    let mut profile = MachineProfile::embedded().unwrap();
    profile.bonus = Some(fixture.bonus);
    profile.dc_motors = fixture.dc_motors;
    let board = profile
        .serial_svmd
        .get_or_insert(SerialSvmdProfile { servos: Vec::new() });
    board.servos.retain(|servo| {
        !fixture
            .serial_svmd
            .servos
            .iter()
            .any(|bonus| bonus.id == servo.id || bonus.name == servo.name)
    });
    board.servos.extend(fixture.serial_svmd.servos);
    profile.validate().unwrap();
    profile
}
