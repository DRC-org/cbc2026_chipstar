use super::*;
use crate::machine::xy;

impl MachineController {
    pub fn xy_blocker(&self, telemetry: Option<&Telemetry>) -> Option<&'static str> {
        let axes = ["r", "theta"].map(|name| self.profile.axes.iter().find(|a| a.name == name));
        if !matches!(axes, [Some(r), Some(t)] if r.unit == "mm" && t.unit == "deg") {
            return Some("XY移動にはr（mm）とθ（deg）の軸設定が必要です");
        }
        let Some(r) = self.axis_position("r", telemetry) else {
            return Some("XY移動にはrの原点と最新の位置が必要です");
        };
        if self
            .axis_position("theta", telemetry)
            .is_none_or(|v| !v.is_finite())
        {
            return Some("XY移動にはθの原点と最新の位置が必要です");
        }
        let radius = r + self.profile.xy.radius_offset_mm;
        if !radius.is_finite() || radius < 1.0 {
            return Some(
                "旋回半径が1 mm未満です。r・θ移動で離すか、r=0の旋回半径を設定してください",
            );
        }
        None
    }

    pub fn jog_at_rest(&self) -> bool {
        self.jog_velocity.iter().all(|v| v.abs() < 0.0001)
    }

    pub fn ramped_xy_jog_lines(
        &mut self,
        input: &ControllerState,
        telemetry: &Telemetry,
        slow: bool,
        now: Instant,
    ) -> Vec<String> {
        let dt = self.jog_tick.replace(now).map_or(0.0, |previous| {
            now.saturating_duration_since(previous)
                .as_secs_f32()
                .min(0.05)
        });
        let velocity = self.xy_jog_velocity(input, telemetry, slow, dt);
        self.jog_lines_with_xy(input, telemetry, slow, Some(dt), Some(velocity))
    }

    fn xy_jog_velocity(
        &mut self,
        input: &ControllerState,
        telemetry: &Telemetry,
        slow: bool,
        dt: f32,
    ) -> [f32; 2] {
        if self.xy_blocker(Some(telemetry)).is_some() || !self.soft_limits {
            self.xy_velocity = [0.0; 2];
            return [0.0; 2];
        }
        let radius =
            self.axis_position("r", Some(telemetry)).unwrap() + self.profile.xy.radius_offset_mm;
        let theta = self.axis_position("theta", Some(telemetry)).unwrap();
        let indices = ["r", "theta"].map(|name| {
            self.profile
                .axes
                .iter()
                .position(|a| a.name == name)
                .unwrap()
        });
        let scale = if slow {
            self.profile.slow_speed_percent * 0.01
        } else {
            1.0
        };
        let desired = xy::stick(input).map(|v| v * self.profile.xy.speed_mm_per_second * scale);
        let desired_joints = xy::joint_velocity(desired, radius, theta);
        let mut desired_factor: f32 = 1.0;
        for (joint, i) in indices.iter().copied().enumerate() {
            desired_factor = desired_factor
                .min(self.profile.axes[i].speed_per_second * scale / desired_joints[joint].abs());
        }
        let desired = desired.map(|v| v * desired_factor);
        // 固定XY座標で加減速し、rとθを別々に立ち上げて進行方向を変えない。
        let mut acceleration = f32::INFINITY;
        for (joint, i) in indices.iter().copied().enumerate() {
            let a = &self.profile.axes[i];
            if a.jog_ramp_seconds > 0.0 {
                let limit = a.speed_per_second / a.jog_ramp_seconds;
                acceleration = acceleration.min(if joint == 0 {
                    limit
                } else {
                    radius * limit.to_radians()
                });
            }
        }
        let delta = [
            desired[0] - self.xy_velocity[0],
            desired[1] - self.xy_velocity[1],
        ];
        let length = delta[0].hypot(delta[1]);
        let fraction = if length == 0.0 || acceleration.is_infinite() {
            1.0
        } else {
            (acceleration * dt / length).min(1.0)
        };
        let world = [
            self.xy_velocity[0] + delta[0] * fraction,
            self.xy_velocity[1] + delta[1] * fraction,
        ];
        let joints = xy::joint_velocity(world, radius, theta);
        // 速度上限・接点・可動域は2軸を同じ比率で制限し、XYの移動方向を保つ。
        let mut factor: f32 = 1.0;
        for (joint, i) in indices.iter().copied().enumerate() {
            let axis = &self.profile.axes[i];
            let requested = joints[joint];
            if requested.abs() < 0.000001 {
                continue;
            }
            // L1で下げた上限は加減速前の要求に適用済み。減速途中の速度を切り落とさない。
            let capped = requested.clamp(-axis.speed_per_second, axis.speed_per_second);
            let allowed = if axis.jog_ramp_seconds > 0.0 {
                self.braking_velocity(i, capped, telemetry)
            } else {
                self.constrain_velocity(i, capped, telemetry, true)
            };
            factor = factor.min((allowed / requested).clamp(0.0, 1.0));
        }
        self.xy_velocity = world.map(|v| v * factor);
        joints.map(|v| if v.abs() < 0.000001 { 0.0 } else { v * factor })
    }
}

#[cfg(test)]
mod tests;
