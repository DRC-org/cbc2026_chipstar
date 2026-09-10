//! 操作権と入力の期限。通信・出力・画面の状態は所有しない。
use crate::input::ControllerState;
use anyhow::{Result, bail};
use std::time::{Duration, Instant};

const INPUT_TTL: Duration = Duration::from_millis(150);
const LEASE_TTL: Duration = Duration::from_secs(30);

struct Lease {
    token: String,
    contact: Instant,
    input: ControllerState,
    input_times: [Option<Instant>; crate::input::MACHINE_INPUT_COUNT],
}

#[derive(Default)]
pub(super) struct Authority {
    lease: Option<Lease>,
}

impl Authority {
    pub fn active(&self) -> bool {
        self.lease.is_some()
    }

    pub fn claim(&mut self, token: String, now: Instant) {
        self.lease = Some(Lease {
            token,
            contact: now,
            input: ControllerState::default(),
            input_times: [None; crate::input::MACHINE_INPUT_COUNT],
        });
    }

    pub fn authorize(
        &mut self,
        token: Option<&str>,
        manual: bool,
        stop: bool,
        now: Instant,
    ) -> Result<()> {
        if stop {
            return Ok(());
        }
        match (&mut self.lease, manual) {
            (None, true) => Ok(()),
            (Some(_), true) => bail!("AI操作中です。停止は常に操作できます"),
            (Some(lease), false) if token == Some(lease.token.as_str()) => {
                lease.contact = now;
                Ok(())
            }
            _ => bail!("claimで操作権を取得し、tokenを指定してください"),
        }
    }

    pub fn release(&mut self) {
        self.lease = None;
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.lease
            .as_ref()
            .is_some_and(|lease| now.duration_since(lease.contact) > LEASE_TTL)
    }

    pub fn clear_input(&mut self) {
        if let Some(lease) = &mut self.lease {
            lease.input = ControllerState::default();
            lease.input_times = [None; crate::input::MACHINE_INPUT_COUNT];
        }
    }

    pub fn input(&self) -> Option<&ControllerState> {
        self.lease.as_ref().map(|lease| &lease.input)
    }

    pub fn set_input(&mut self, index: usize, value: f32, now: Instant) {
        if let Some(lease) = &mut self.lease {
            lease.input.set_machine_axis(index, value);
            lease.input_times[index] = Some(now);
        }
    }

    pub fn expire_inputs(&mut self, now: Instant) {
        if let Some(lease) = &mut self.lease {
            for (index, stamp) in lease.input_times.iter_mut().enumerate() {
                if stamp.is_some_and(|time| now.duration_since(time) > INPUT_TTL) {
                    lease.input.set_machine_axis(index, 0.0);
                    *stamp = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_renews_ownership_but_not_stick_input() {
        let now = Instant::now();
        let mut authority = Authority::default();
        authority.claim("test".into(), now);
        authority.set_input(0, 0.5, now);
        authority.set_input(1, -0.5, now + INPUT_TTL);
        let later = now + INPUT_TTL + Duration::from_millis(1);
        authority
            .authorize(Some("test"), false, false, later)
            .unwrap();
        authority.expire_inputs(later);
        assert_eq!(authority.input().unwrap().axes[0], 0.0);
        assert_eq!(authority.input().unwrap().axes[1], -0.5);
        assert!(!authority.expired(later + LEASE_TTL));
        assert!(authority.expired(later + LEASE_TTL + Duration::from_millis(1)));
    }

    #[test]
    fn stop_is_unconditional_and_clearing_input_keeps_ownership() {
        let now = Instant::now();
        let mut authority = Authority::default();
        authority.claim("test".into(), now);
        assert!(authority.authorize(None, true, false, now).is_err());
        assert!(
            authority
                .authorize(Some("wrong"), false, false, now)
                .is_err()
        );
        assert!(authority.authorize(None, true, true, now).is_ok());
        assert!(authority.authorize(None, false, true, now).is_ok());
        authority.set_input(0, 1.0, now);
        authority.clear_input();
        assert!(authority.active());
        assert_eq!(authority.input().unwrap().axes, [0.0; 6]);
        authority.release();
        assert!(authority.input().is_none());
        assert!(authority.authorize(None, true, false, now).is_ok());
    }
}
