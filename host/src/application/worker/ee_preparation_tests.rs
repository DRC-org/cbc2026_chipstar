use super::*;

fn prepared() -> (Runtime, Instant) {
    let mut r = crate::application::worker::tests::screen_runtime();
    r.cfg.machine.serial_svmd = MachineProfile::embedded().unwrap().serial_svmd;
    let now = Instant::now();
    r.test.peers.insert("sts", now);
    r.servo_feedback.insert(
        1,
        ServoFeedback {
            absolute_position: true,
            seen: now,
            position: 1500,
            error: 0,
            detail: String::new(),
        },
    );
    r.ee.prepared_rotation_field = Some(12.0);
    r.ee.preparation_hold_since = Some(now);
    r.front_return_pending = Some(180.0);
    (r, now)
}

#[test]
fn prepared_hold_tracks_theta_and_survives_waiting_and_front_return_start() {
    let (mut r, now) = prepared();
    r.tick_ee(now).unwrap();
    assert_eq!(r.ee.rotation_field, 12.0);
    assert_eq!(r.ee.targets.len(), 1);
    assert!(r.ee.prepared_rotation_field.is_none());
    assert!(!r.drive.running());
    r.request(&Request::new("preparation_wait"), true).unwrap();
    assert_eq!(r.preparation, PreparationPhase::Waiting);
    let theta = r
        .cfg
        .machine
        .axes
        .iter()
        .find(|a| a.name == "theta")
        .unwrap()
        .clone();
    r.telemetry.as_mut().unwrap().slots[theta.slot as usize].measured +=
        10.0 * theta.native_per_unit;
    r.tick_ee(now + Duration::from_millis(60)).unwrap();
    let axis = ee::axes(&r.cfg.machine)
        .into_iter()
        .find(|a| a.name == "ee_rotation")
        .unwrap();
    let expected = f32::from(axis.rotation_count(12.0, 10.0).unwrap());
    assert_eq!(r.ee.targets["ee_rotation"], expected);

    // 競技開始のRUN応答前・正面復帰中も、すでに有効なEEを再初期化しない。
    r.request(&Request::new("preparation_start"), true).unwrap();
    assert!(r.drive.awaiting().is_some());
    assert!(r.homing.as_ref().unwrap().is_front_return());
    let before = r.shared.status_snapshot().logs.len();
    r.servo_feedback.get_mut(&1).unwrap().position = 2000;
    r.tick_ee(now + Duration::from_millis(80)).unwrap();
    assert_eq!(r.ee.rotation_field, 12.0);
    assert_eq!(r.ee.targets["ee_rotation"], expected);
    r.drive = DriveState::Running;
    r.tick_ee(now + Duration::from_millis(100)).unwrap();
    assert!(r.ee.preparation_hold_since.is_none());
    assert_eq!(r.ee.rotation_field, 12.0);
    assert!(
        r.shared
            .status_snapshot()
            .logs
            .iter()
            .skip(before)
            .all(|line| line != "TX CAN 2 800 0106010100000000"
                && line != "TX CAN 2 800 0102000000000000")
    );
}

#[test]
fn stop_restart_and_emergency_cancel_prepared_hold_without_automatic_reenable() {
    for action in ["stop", "preparation_restart", "estop"] {
        for already_holding in [false, true] {
            let (mut r, now) = prepared();
            if already_holding {
                r.tick_ee(now).unwrap();
            }
            r.request(&Request::new(action), true).unwrap();
            assert!(r.ee.targets.is_empty());
            assert!(r.ee.prepared_rotation_field.is_none());
            assert!(r.ee.preparation_hold_since.is_none());
            let before = r.shared.status_snapshot().logs.clone();
            r.tick_ee(now + Duration::from_millis(60)).unwrap();
            assert_eq!(r.shared.status_snapshot().logs, before);
            if action == "stop" {
                r.servo_feedback.get_mut(&1).unwrap().position = 1700;
                r.drive = DriveState::Running;
                r.tick_ee(now + Duration::from_millis(80)).unwrap();
                assert_eq!(r.ee.targets["ee_rotation"], 1700.0);
            }
        }
    }
}

#[test]
fn prepared_hold_rejects_lost_origin_or_feedback_before_output() {
    for case in 0..5 {
        let (mut r, now) = prepared();
        match case {
            0 => r.machine.invalidate_origin(1),
            1 => r.telemetry.as_mut().unwrap().stale_slots = 2,
            2 => r.servo_feedback.get_mut(&1).unwrap().absolute_position = false,
            3 => r.emergency = true,
            _ => r.test.enabled = true,
        }
        let before = r.shared.status_snapshot().logs.clone();
        assert!(r.tick_ee(now).is_err());
        assert_eq!(r.shared.status_snapshot().logs, before);
        assert!(r.ee.targets.is_empty());
    }
}

#[test]
fn prepared_hold_times_out_without_servo_response_and_fault_cancels_it() {
    let (mut r, now) = prepared();
    r.servo_feedback.clear();
    let before = r.shared.status_snapshot().logs.clone();
    r.tick_ee(now).unwrap();
    assert_eq!(r.shared.status_snapshot().logs, before);
    let error = r.tick_ee(now + Duration::from_millis(501)).unwrap_err();
    r.fault_ee(error.to_string());
    assert!(r.ee.preparation_hold_since.is_none());
    assert!(r.ee.prepared_rotation_field.is_none());
    assert!(r.ee.targets.is_empty());
    assert!(!r.drive.running());
    let before = r.shared.status_snapshot().logs.clone();
    r.tick_ee(now + Duration::from_millis(600)).unwrap();
    assert_eq!(r.shared.status_snapshot().logs, before);
}

#[test]
fn waiting_for_ee_readback_can_be_cancelled_by_communication_loss() {
    let (mut r, now) = prepared();
    r.communication_failed("test link loss".into());
    assert!(r.ee.prepared_rotation_field.is_none());
    assert!(r.ee.preparation_hold_since.is_none());
    let before = r.shared.status_snapshot().logs.clone();
    r.tick_ee(now + Duration::from_millis(60)).unwrap();
    assert_eq!(r.shared.status_snapshot().logs, before);
}

#[test]
fn controller_disconnect_cancels_prepared_hold_before_and_after_first_target() {
    for already_holding in [false, true] {
        let (mut r, now) = prepared();
        r.screen_control = false;
        if already_holding {
            r.tick_ee(now).unwrap();
        }
        r.disconnect_pad();
        assert!(r.ee.prepared_rotation_field.is_none());
        assert!(r.ee.preparation_hold_since.is_none());
        assert!(r.ee.targets.is_empty());
        let before = r.shared.status_snapshot().logs.clone();
        r.tick_ee(now + Duration::from_millis(60)).unwrap();
        assert_eq!(r.shared.status_snapshot().logs, before);
    }
}
