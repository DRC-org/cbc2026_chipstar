//! 静止位置間を、速度と加減速時間に従って補間する。

#[derive(Clone, Copy)]
pub struct PositionProfile {
    from: f32,
    to: f32,
    direction: f32,
    speed: f32,
    acceleration: f32,
    acceleration_seconds: f32,
    total_seconds: f32,
}

impl PositionProfile {
    /// 有限の位置、正の最高速度、0以上の加減速時間を渡す。
    /// 加減速時間が0の場合も、最高速度で位置を補間する。
    pub fn new(from: f32, to: f32, max_speed: f32, ramp_seconds: f32) -> Self {
        let distance = (to - from).abs();
        let (speed, acceleration, acceleration_seconds, total_seconds) = if distance == 0.0 {
            (0.0, 0.0, 0.0, 0.0)
        } else if ramp_seconds == 0.0 {
            (max_speed, 0.0, 0.0, distance / max_speed)
        } else {
            let acceleration = max_speed / ramp_seconds;
            let acceleration_seconds = ramp_seconds.min((distance / acceleration).sqrt());
            let speed = acceleration * acceleration_seconds;
            let cruise_seconds = ((distance - speed * acceleration_seconds) / speed).max(0.0);
            (
                speed,
                acceleration,
                acceleration_seconds,
                2.0 * acceleration_seconds + cruise_seconds,
            )
        };
        Self {
            from,
            to,
            direction: (to - from).signum(),
            speed,
            acceleration,
            acceleration_seconds,
            total_seconds,
        }
    }

    /// 開始前は始点、完了後は終点を返す。
    pub fn position(&self, elapsed_seconds: f32) -> f32 {
        if elapsed_seconds <= 0.0 {
            return self.from;
        }
        if elapsed_seconds >= self.total_seconds {
            return self.to;
        }
        if elapsed_seconds < self.acceleration_seconds {
            self.from + self.direction * 0.5 * self.acceleration * elapsed_seconds * elapsed_seconds
        } else if elapsed_seconds < self.total_seconds - self.acceleration_seconds {
            self.from
                + self.direction * self.speed * (elapsed_seconds - 0.5 * self.acceleration_seconds)
        } else {
            let remaining = self.total_seconds - elapsed_seconds;
            self.to - self.direction * 0.5 * self.acceleration * remaining * remaining
        }
    }

    pub fn duration(&self) -> f32 {
        self.total_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }

    #[test]
    fn long_move_accelerates_cruises_and_stops_at_destination() {
        let profile = PositionProfile::new(10.0, 110.0, 20.0, 1.0);
        close(profile.duration(), 6.0);
        for (seconds, position) in [
            (-1.0, 10.0),
            (0.0, 10.0),
            (0.5, 12.5),
            (1.0, 20.0),
            (3.0, 60.0),
            (5.0, 100.0),
            (5.5, 107.5),
            (6.0, 110.0),
            (7.0, 110.0),
        ] {
            close(profile.position(seconds), position);
        }
    }

    #[test]
    fn short_move_uses_a_triangle_without_reaching_maximum_speed() {
        let profile = PositionProfile::new(0.0, 4.0, 10.0, 1.0);
        let half_time = (4.0_f32 / 10.0).sqrt();
        close(profile.duration(), 2.0 * half_time);
        close(profile.position(half_time * 0.5), 0.5);
        close(profile.position(half_time), 2.0);
        close(profile.position(half_time * 1.5), 3.5);
        close(profile.position(profile.duration()), 4.0);
    }

    #[test]
    fn reverse_move_uses_the_same_timing_and_mirrored_positions() {
        let forward = PositionProfile::new(10.0, 110.0, 20.0, 1.0);
        let reverse = PositionProfile::new(110.0, 10.0, 20.0, 1.0);
        close(reverse.duration(), forward.duration());
        for seconds in [0.0, 0.5, 1.0, 3.0, 5.0, 5.5, 6.0, 7.0] {
            close(reverse.position(seconds), 120.0 - forward.position(seconds));
        }
    }

    #[test]
    fn zero_ramp_moves_at_maximum_speed_without_jumping_to_the_target() {
        let profile = PositionProfile::new(100.0, 0.0, 20.0, 0.0);
        close(profile.duration(), 5.0);
        close(profile.position(0.0), 100.0);
        close(profile.position(0.5), 90.0);
        close(profile.position(2.5), 50.0);
        close(profile.position(5.0), 0.0);
        close(profile.position(6.0), 0.0);
    }

    #[test]
    fn no_distance_is_already_at_rest() {
        for ramp in [0.0, 0.7] {
            let profile = PositionProfile::new(12.0, 12.0, 20.0, ramp);
            close(profile.duration(), 0.0);
            for seconds in [-1.0, 0.0, 1.0] {
                close(profile.position(seconds), 12.0);
            }
        }
    }

    #[test]
    fn sampled_motion_respects_speed_acceleration_and_monotonicity() {
        let max_speed = 13.0;
        let ramp_seconds = 0.7;
        let dt = 0.05;
        for to in [3.0_f32, -3.0, 101.0, -101.0] {
            let profile = PositionProfile::new(0.0, to, max_speed, ramp_seconds);
            let mut previous_position = 0.0;
            let mut previous_speed = 0.0;
            let mut peak_speed = 0.0_f32;
            for step in 1..=(profile.duration() / dt).ceil() as usize + 2 {
                let position = profile.position(step as f32 * dt);
                let speed = (position - previous_position) / dt;
                assert!(speed * to.signum() >= -0.0001);
                assert!(position.abs() <= to.abs() + 0.0001);
                assert!(speed.abs() <= max_speed + 0.001);
                assert!((speed - previous_speed).abs() / dt <= max_speed / ramp_seconds + 0.02);
                peak_speed = peak_speed.max(speed.abs());
                previous_position = position;
                previous_speed = speed;
            }
            close(previous_position, to);
            close(previous_speed, 0.0);
            if to.abs() > 100.0 {
                assert!(peak_speed > max_speed - 0.001);
            } else {
                assert!(peak_speed < max_speed);
            }
        }
    }
}
