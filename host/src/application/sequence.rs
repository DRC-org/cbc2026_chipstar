//! 取得グループ、受渡し姿勢と、順序・同時動作を編集できるシーケンス。
use crate::machine::{MachineProfile, ee};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepare,
    Pick,
    Transfer,
    All,
}
impl Stage {
    pub const ALL: [Self; 4] = [Self::Prepare, Self::Pick, Self::Transfer, Self::All];
    pub fn label(self) -> &'static str {
        match self {
            Self::Prepare => "取得準備",
            Self::Pick => "取得",
            Self::Transfer => "受け渡し",
            Self::All => "３操作を連続実行",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}
impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "左",
            Self::Right => "右",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    WorkPosition,
    WorkRotation,
    Unfold,
    Open,
    Approach,
    Descend,
    Close,
    Lift,
    TravelHeight,
    BonusPosition,
    Fold,
    BonusRotation,
    BonusHeight,
    HandoffOpen,
    LoadedRetreat,
}
impl Action {
    pub const ALL: [Self; 15] = [
        Self::WorkPosition,
        Self::WorkRotation,
        Self::Unfold,
        Self::Open,
        Self::Approach,
        Self::Descend,
        Self::Close,
        Self::Lift,
        Self::TravelHeight,
        Self::BonusPosition,
        Self::Fold,
        Self::BonusRotation,
        Self::BonusHeight,
        Self::HandoffOpen,
        Self::LoadedRetreat,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::WorkPosition => "ワーク上空へ移動",
            Self::WorkRotation => "取得向きへEE回転",
            Self::Unfold => "EE展開",
            Self::Open => "把持開",
            Self::Approach => "取得高さへ下降",
            Self::Descend => "取得高さへ下降（旧工程）",
            Self::Close => "把持閉",
            Self::Lift => "確認用に小上昇（旧工程）",
            Self::TravelHeight => "ワークあり移動高さへ上昇",
            Self::BonusPosition => "受け渡し場所上空へ移動",
            Self::Fold => "EEたたみ",
            Self::BonusRotation => "受渡し向きへEE回転",
            Self::BonusHeight => "外枠内へ段階下降",
            Self::HandoffOpen => "受け渡しレベルまで把持開",
            Self::LoadedRetreat => "ワークあり移動高さへ垂直退避",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    pub actions: Vec<Action>,
    /// 同じ工程の全動作を開始してから待つ時間。アームは到達も待つ。
    pub wait_seconds: f32,
}
impl Step {
    pub fn new(action: Action) -> Self {
        Self {
            name: action.label().into(),
            actions: vec![action],
            wait_seconds: 0.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    pub r: f32,
    pub theta: f32,
    pub approach_z: f32,
    pub grab_z: f32,
    /// 先端回転のフィールド基準角[deg]。0°または180°。
    pub rotation: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub r: f32,
    pub theta: f32,
    pub z: f32,
    /// 先端回転のフィールド基準角[deg]。0°または180°。
    pub rotation: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub groups: Vec<Group>,
    pub left: Destination,
    pub right: Destination,
    /// ワークを持たない状態でr・θ移動に使うz高さ。
    pub travel_z: f32,
    /// ワークを持った状態でr・θ移動と垂直退避に使うz高さ。
    #[serde(default)]
    pub loaded_travel_z: f32,
    /// 受け渡し場所で外枠へ入れるときの1段あたりz移動量。
    #[serde(default = "default_handoff_descent_step_mm")]
    pub handoff_descent_step_mm: f32,
    /// 旧設定ファイルの読込み用。標準工程では使用しない。
    pub lift_mm: f32,
    pub unfolded: f32,
    pub folded: f32,
    pub grip_open: [f32; 3],
    pub grip_closed: [f32; 3],
    #[serde(default = "default_grip_level")]
    pub grip_handoff_open: [f32; 3],
    pub speed_percent: f32,
    pub tolerance_mm: f32,
    pub tolerance_deg: f32,
    pub timeout_seconds: f32,
    pub prepare: Vec<Step>,
    pub pick: Vec<Step>,
    pub transfer: Vec<Step>,
}
fn default_handoff_descent_step_mm() -> f32 {
    5.0
}
fn default_grip_level() -> [f32; 3] {
    [1500.0; 3]
}
impl Config {
    pub fn from_machine(machine: &MachineProfile) -> Self {
        let position = |name: &str| {
            machine
                .axes
                .iter()
                .find(|a| a.name == name)
                .map_or(0.0, |a| a.initial)
        };
        let axes = ee::axes(machine);
        let servo = |name: &str, fallback| {
            axes.iter()
                .find(|a| a.name == name)
                .map_or(fallback, |a| a.initial)
        };
        // 回転はフィールド基準角[deg]。既定は0°で、工程では0°/180°を選ぶ。
        let rotation = 0.0;
        let grips = std::array::from_fn(|i| servo(&format!("ee_grip_{}", i + 1), 1500.0));
        let destination = Destination {
            r: position("r"),
            theta: position("theta"),
            z: position("z"),
            rotation,
        };
        let steps = |actions: &[Action]| {
            actions
                .iter()
                .map(|action| {
                    let mut step = Step::new(*action);
                    if matches!(
                        action,
                        Action::WorkRotation
                            | Action::Unfold
                            | Action::Open
                            | Action::Close
                            | Action::Fold
                            | Action::BonusRotation
                    ) {
                        step.wait_seconds = 0.5;
                    }
                    if *action == Action::Fold {
                        step.wait_seconds = 2.0;
                    }
                    step
                })
                .collect()
        };
        let prepare = steps(&[
            Action::WorkPosition,
            Action::Unfold,
            Action::WorkRotation,
            Action::Open,
            Action::Approach,
        ]);
        let transfer = steps(&[
            Action::Fold,
            Action::BonusHeight,
            Action::HandoffOpen,
            Action::LoadedRetreat,
        ]);
        Self {
            groups: (1..=8)
                .map(|i| Group {
                    name: format!("グループ{i}"),
                    r: position("r"),
                    theta: position("theta"),
                    approach_z: position("z"),
                    grab_z: position("z"),
                    rotation,
                })
                .collect(),
            left: destination.clone(),
            right: destination,
            travel_z: position("z"),
            loaded_travel_z: position("z"),
            handoff_descent_step_mm: default_handoff_descent_step_mm(),
            lift_mm: 10.0,
            unfolded: servo("ee_fold", 1500.0),
            folded: servo("ee_fold", 1500.0),
            grip_open: grips,
            grip_closed: [700.0; 3],
            grip_handoff_open: [500.0; 3],
            speed_percent: 50.0,
            tolerance_mm: 1.0,
            tolerance_deg: 1.0,
            timeout_seconds: 60.0,
            prepare,
            pick: steps(&[Action::Close, Action::TravelHeight]),
            transfer,
        }
    }
    pub fn steps(&self, stage: Stage) -> &[Step] {
        match stage {
            Stage::Prepare => &self.prepare,
            Stage::Pick => &self.pick,
            Stage::Transfer => &self.transfer,
            Stage::All => &[],
        }
    }
    pub fn steps_mut(&mut self, stage: Stage) -> &mut Vec<Step> {
        match stage {
            Stage::Prepare => &mut self.prepare,
            Stage::Pick => &mut self.pick,
            Stage::Transfer => &mut self.transfer,
            Stage::All => unreachable!("連続実行は３操作の連結"),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.groups.is_empty(),
            "取得グループを１つ以上登録してください"
        );
        ensure!(
            self.speed_percent.is_finite()
                && self.speed_percent > 0.0
                && self.speed_percent <= 100.0,
            "シーケンス速度率は0より大きく100以下で指定してください"
        );
        for (value, label) in [
            (self.tolerance_mm, "mm許容差"),
            (self.tolerance_deg, "角度許容差"),
            (self.timeout_seconds, "工程制限時間"),
        ] {
            ensure!(
                value.is_finite() && value > 0.0,
                "{label}は正の数で指定してください"
            );
        }
        ensure!(
            self.lift_mm.is_finite() && self.lift_mm >= 0.0,
            "小上昇量は0以上で指定してください"
        );
        ensure!(
            self.handoff_descent_step_mm.is_finite() && self.handoff_descent_step_mm > 0.0,
            "受け渡し下降刻みは0より大きい値で指定してください"
        );
        for group in 0..self.groups.len() {
            for side in [Side::Left, Side::Right] {
                self.resolve(&Start {
                    group,
                    side,
                    stage: Stage::All,
                })?;
            }
        }
        Ok(())
    }
    pub fn resolve(&self, start: &Start) -> Result<Vec<ResolvedStep>> {
        let group = self
            .groups
            .get(start.group)
            .context("取得グループが見つかりません")?;
        let dest = match start.side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        };
        let stages = if start.stage == Stage::All {
            vec![Stage::Prepare, Stage::Pick, Stage::Transfer]
        } else {
            vec![start.stage]
        };
        let mut result = Vec::new();
        for stage in stages {
            let mut steps = self.steps(stage).to_vec();
            // 個別の取得・受け渡し間では操作者がr・θを合わせる。
            // 連続実行だけは登録した受け渡し上空位置へ自動移動する。
            if start.stage == Stage::All
                && stage == Stage::Transfer
                && !steps
                    .iter()
                    .any(|step| step.actions.contains(&Action::BonusPosition))
            {
                steps.insert(0, Step::new(Action::BonusPosition));
            }
            for step in &steps {
                ensure!(
                    step.wait_seconds.is_finite() && step.wait_seconds >= 0.0,
                    "工程の待ち時間は0以上で指定してください"
                );
                if step.actions.contains(&Action::BonusHeight) {
                    ensure!(
                        step.actions == [Action::BonusHeight],
                        "外枠内への段階下降は単独工程にしてください"
                    );
                    let distance = dest.z - self.loaded_travel_z;
                    let count = (distance.abs() / self.handoff_descent_step_mm)
                        .ceil()
                        .max(1.0) as usize;
                    ensure!(count <= 1000, "受け渡し下降刻みが細かすぎます");
                    for index in 1..=count {
                        let z = self.loaded_travel_z + distance * index as f32 / count as f32;
                        result.push(ResolvedStep {
                            name: format!("{} / {} {index}/{count}", stage.label(), step.name),
                            axes: BTreeMap::from([("z".into(), z)]),
                            ee: BTreeMap::new(),
                            wait_seconds: if index == count {
                                step.wait_seconds
                            } else {
                                0.0
                            },
                        });
                    }
                    continue;
                }
                let mut resolved = ResolvedStep {
                    name: format!("{} / {}", stage.label(), step.name),
                    axes: BTreeMap::new(),
                    ee: BTreeMap::new(),
                    wait_seconds: step.wait_seconds,
                };
                for action in &step.actions {
                    let (arm, values): (bool, Vec<(&str, f32)>) = match action {
                        Action::WorkPosition => (
                            true,
                            vec![("r", group.r), ("theta", group.theta), ("z", self.travel_z)],
                        ),
                        Action::WorkRotation => (false, vec![("ee_rotation", group.rotation)]),
                        Action::Unfold => (false, vec![("ee_fold", self.unfolded)]),
                        Action::Fold => (false, vec![("ee_fold", self.folded)]),
                        Action::Open | Action::Close | Action::HandoffOpen => {
                            let values = match action {
                                Action::Open => self.grip_open,
                                Action::Close => self.grip_closed,
                                Action::HandoffOpen => self.grip_handoff_open,
                                _ => unreachable!(),
                            };
                            (
                                false,
                                vec![
                                    ("ee_grip_1", values[0]),
                                    ("ee_grip_2", values[1]),
                                    ("ee_grip_3", values[2]),
                                ],
                            )
                        }
                        Action::Approach => (true, vec![("z", group.grab_z)]),
                        Action::Descend => (true, vec![("z", group.grab_z)]),
                        Action::Lift => (true, vec![("z", group.grab_z + self.lift_mm)]),
                        Action::TravelHeight | Action::LoadedRetreat => {
                            (true, vec![("z", self.loaded_travel_z)])
                        }
                        Action::BonusPosition => (
                            true,
                            vec![
                                ("r", dest.r),
                                ("theta", dest.theta),
                                ("z", self.loaded_travel_z),
                            ],
                        ),
                        Action::BonusRotation => (false, vec![("ee_rotation", dest.rotation)]),
                        Action::BonusHeight => unreachable!("段階下降は上で展開済み"),
                    };
                    let map = if arm {
                        &mut resolved.axes
                    } else {
                        &mut resolved.ee
                    };
                    for (name, value) in values {
                        ensure!(
                            value.is_finite(),
                            "{}の{name}に有限の数を指定してください",
                            step.name
                        );
                        ensure!(
                            map.insert(name.into(), value).is_none(),
                            "{}で{name}への指令が重複しています",
                            step.name
                        );
                    }
                }
                result.push(resolved);
            }
        }
        Ok(result)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub group: usize,
    pub side: Side,
    pub stage: Stage,
}
#[derive(Clone, Debug)]
pub struct ResolvedStep {
    pub name: String,
    pub axes: BTreeMap<String, f32>,
    pub ee: BTreeMap<String, f32>,
    pub wait_seconds: f32,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub active: bool,
    pub group: String,
    pub side: String,
    pub step: usize,
    pub total: usize,
    pub message: String,
}
pub fn config_path(profile: &Path) -> PathBuf {
    profile.with_extension("sequences.toml")
}
pub fn load(path: &Path) -> Result<Config> {
    let mut value: toml::Value = toml::from_str(&std::fs::read_to_string(path)?)?;
    if let Some(table) = value.as_table_mut() {
        if !table.contains_key("loaded_travel_z")
            && let Some(travel) = table.get("travel_z").cloned()
        {
            table.insert("loaded_travel_z".into(), travel);
        }
        if !table.contains_key("grip_handoff_open")
            && let Some(open) = table.get("grip_open").cloned()
        {
            table.insert("grip_handoff_open".into(), open);
        }
    }
    let config: Config = value.try_into()?;
    config.validate()?;
    Ok(config)
}
pub fn save(path: &Path, config: &Config) -> Result<()> {
    config.validate()?;
    crate::transport::profile_store::save_text(path, &toml::to_string_pretty(config)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipes_resolve_group_side_lift_and_parallel_steps() {
        let mut config = Config::from_machine(&MachineProfile::embedded().unwrap());
        config.groups[1].r = 120.0;
        config.groups[1].grab_z = 25.0;
        config.travel_z = 40.0;
        config.loaded_travel_z = 33.0;
        config.handoff_descent_step_mm = 2.0;
        config.left.rotation = 0.0;
        config.right.rotation = 180.0;
        config.right.r = 210.0;
        config.right.z = 28.0;
        config.grip_handoff_open = [1200.0, 1201.0, 1202.0];
        config.prepare[0].actions.push(Action::WorkRotation);
        let request = Start {
            group: 1,
            side: Side::Right,
            stage: Stage::All,
        };
        let steps = config.resolve(&request).unwrap();
        assert_eq!(steps[0].axes["r"], 120.0);
        assert!(steps[0].ee.contains_key("ee_rotation"));
        assert!(steps.iter().any(|s| s.axes.get("z") == Some(&33.0)));
        assert!(steps.iter().any(|s| s.ee.get("ee_grip_2") == Some(&1201.0)));
        assert!(steps.iter().any(|s| s.axes.get("r") == Some(&210.0)));
        let descent: Vec<_> = steps
            .iter()
            .filter(|step| step.name.contains("外枠内へ段階下降"))
            .map(|step| step.axes["z"])
            .collect();
        assert_eq!(descent, [31.333334, 29.666666, 28.0]);
        assert_eq!(steps.last().unwrap().axes["z"], 33.0);
        let transfer = config
            .resolve(&Start {
                group: 1,
                side: Side::Right,
                stage: Stage::Transfer,
            })
            .unwrap();
        assert!(
            transfer
                .iter()
                .all(|step| !step.axes.contains_key("r") && !step.axes.contains_key("theta"))
        );
        assert_eq!(
            Config::from_machine(&MachineProfile::embedded().unwrap()).grip_closed,
            [700.0; 3]
        );
        assert_eq!(
            Config::from_machine(&MachineProfile::embedded().unwrap()).grip_handoff_open,
            [500.0; 3]
        );
        let encoded = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&encoded).unwrap(), config);
        config.prepare[0].actions.push(Action::BonusPosition);
        assert!(config.validate().is_err());
    }
    #[test]
    fn saved_config_preserves_teaching_and_recipe_order() {
        let root = std::env::temp_dir().join(format!(
            "catchrobo-sequence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = config_path(&root.join("machine.toml"));
        let mut config = Config::from_machine(&MachineProfile::embedded().unwrap());
        config.groups[0].r = 123.0;
        config.prepare.swap(1, 2);
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap(), config);
        config.lift_mm = 12.0;
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap().lift_mm, 12.0);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
