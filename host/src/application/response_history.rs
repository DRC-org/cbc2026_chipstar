//! 受信時に記録する軸応答。画面更新やログの保持件数に依存しない。
use crate::{
    machine::{MachineProfile, OriginState},
    protocol::telemetry::Telemetry,
};
use std::{collections::VecDeque, sync::Arc, time::Instant};

#[derive(Clone, Debug)]
pub struct AxisSample {
    pub name: String,
    pub unit: String,
    pub slot: u8,
    pub target: f32,
    pub measured: f32,
    pub valid: bool,
    pub enabled: bool,
    pub origin_confirmed: bool,
    pub scale: f32,
    pub offset: f32,
}
#[derive(Clone, Debug)]
pub struct Sample {
    pub seconds: f64,
    pub board_ms: u32,
    pub received_seconds: f64,
    pub axes: Vec<AxisSample>,
}
pub struct History {
    epoch: Instant,
    pub samples: VecDeque<Arc<Sample>>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            samples: VecDeque::new(),
        }
    }
}
impl History {
    pub fn record(&mut self, t: &Telemetry, profile: &MachineProfile, origins: &[OriginState]) {
        self.record_at(t, profile, origins, self.epoch.elapsed().as_secs_f64());
    }
    fn record_at(
        &mut self,
        t: &Telemetry,
        profile: &MachineProfile,
        origins: &[OriginState],
        seconds: f64,
    ) {
        if self
            .samples
            .back()
            .is_some_and(|old| old.board_ms == t.uptime_ms)
        {
            return;
        }
        let received_seconds = seconds;
        let seconds = self.samples.back().map_or(seconds, |old| {
            let delta = t.uptime_ms.wrapping_sub(old.board_ms);
            if delta < 0x80000000 {
                old.seconds + f64::from(delta) / 1000.0
            } else {
                seconds.max(old.seconds + 0.001)
            }
        });
        let axes = profile
            .axes
            .iter()
            .filter_map(|axis| {
                let state = t.slots.get(usize::from(axis.slot))?;
                let origin = origins.iter().find(|o| o.name == axis.name)?;
                let measured = origin.position;
                let target = measured + (state.target - state.measured) / axis.native_per_unit;
                Some(AxisSample {
                    name: axis.name.clone(),
                    unit: axis.unit.clone(),
                    slot: axis.slot,
                    target,
                    measured,
                    valid: t.stale_slots & (1 << axis.slot) == 0
                        && t.error_bits[usize::from(axis.slot)] == 0
                        && target.is_finite()
                        && measured.is_finite(),
                    enabled: t.mode == crate::protocol::telemetry::RunMode::Run
                        && t.enabled_slots & (1 << axis.slot) != 0,
                    origin_confirmed: origin.captured && !origin.lost,
                    scale: axis.native_per_unit,
                    offset: state.measured - measured * axis.native_per_unit,
                })
            })
            .collect();
        self.samples.push_back(Arc::new(Sample {
            seconds,
            board_ms: t.uptime_ms,
            received_seconds,
            axes,
        }));
        while self.samples.len() > 6000
            || self
                .samples
                .front()
                .is_some_and(|s| received_seconds - s.received_seconds > 120.0)
        {
            self.samples.pop_front();
        }
    }
}
/// 欠測、再起動、原点・換算・軸割当変更を直線で結ばない。
pub fn continuous(a: &Sample, x: &AxisSample, b: &Sample, y: &AxisSample) -> bool {
    let dt = b.board_ms.wrapping_sub(a.board_ms);
    x.valid
        && y.valid
        && dt > 0
        && dt <= 250
        && b.seconds - a.seconds <= 0.25
        && b.received_seconds - a.received_seconds <= 0.25
        && x.enabled == y.enabled
        && x.origin_confirmed == y.origin_confirmed
        && x.slot == y.slot
        && x.unit == y.unit
        && x.scale == y.scale
        && (x.offset - y.offset).abs() < x.scale.abs() * 0.001 + 0.001
}
pub fn csv(samples: &VecDeque<Arc<Sample>>) -> String {
    let mut out = String::from(
        "board_seconds,received_seconds,board_ms,axis,unit,slot,target,measured,error,valid,enabled,origin_confirmed\n",
    );
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for s in samples {
        for a in &s.axes {
            out.push_str(&format!(
                "{:.6},{:.6},{},{},{},{},{},{},{},{},{},{}\n",
                s.seconds,
                s.received_seconds,
                s.board_ms,
                quote(&a.name),
                quote(&a.unit),
                a.slot,
                a.target,
                a.measured,
                a.target - a.measured,
                a.valid,
                a.enabled,
                a.origin_confirmed
            ));
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::telemetry::parse_telemetry;
    #[test]
    fn captures_actual_board_target_with_machine_origin_and_signed_scale() {
        let mut p = MachineProfile::embedded().unwrap();
        p.axes.truncate(1);
        p.axes[0].slot = 0;
        p.axes[0].native_per_unit = -2.0;
        let origins = vec![OriginState {
            name: p.axes[0].name.clone(),
            unit: p.axes[0].unit.clone(),
            lost: false,
            captured: true,
            at_limit: None,
            position: 120.0,
            target: 999.0,
        }];
        let mut t =
            parse_telemetry("STATE t=10 mode=RUN en=1 a0=14/10 a1=0/0 a2=0/0 err=00,00,00 stale=0")
                .unwrap();
        let mut h = History::default();
        h.record_at(&t, &p, &origins, 0.0);
        let a = &h.samples[0].axes[0];
        assert_eq!(a.target, 118.0);
        assert_eq!(a.measured, 120.0);
        h.record_at(&t, &p, &origins, 0.01);
        assert_eq!(h.samples.len(), 1);
        t.uptime_ms = 20;
        t.stale_slots = 1;
        h.record_at(&t, &p, &origins, 0.02);
        assert!(!h.samples[1].axes[0].valid);
        assert!(!continuous(
            &h.samples[0],
            &h.samples[0].axes[0],
            &h.samples[1],
            &h.samples[1].axes[0]
        ));
        t.uptime_ms = 30;
        h.record_at(&t, &p, &origins, 121.0);
        assert_eq!(h.samples.len(), 1);
        assert!(csv(&h.samples).contains("118,120,-2,false,true"));
    }
}
