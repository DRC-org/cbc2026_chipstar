//! 基板の公開パラメータの説明。ID・意味はdevice_protocol.mdとFW定義に対応する。
pub(super) fn help(name: &str) -> Option<(&'static str, &'static str)> {
    if let Some(suffix) = name.strip_prefix("m3508_slot2_") {
        return help(&format!("m3508_{suffix}"));
    }
    Some(match name {
        "m3508_pos_kp" => (
            "",
            "位置誤差に比例して戻す強さ。大きくすると追従が強まり、振動しやすくなります。",
        ),
        "m3508_pos_ki" => (
            "",
            "位置誤差の積み残しを補う積分ゲイン。上げ過ぎると行き過ぎが生じます。",
        ),
        "m3508_pos_kd" => (
            "",
            "位置誤差の変化に応じた微分ゲイン。急な変化や計測ノイズにも反応します。",
        ),
        "m3508_vel_kp" => ("", "回転速度の誤差に比例して電流指令を変える強さ。"),
        "m3508_vel_ki" => (
            "",
            "速度誤差を秒単位で積分するゲイン（mA/(rpm・s)）。ミリ秒基準の値からは1000倍に換算します。",
        ),
        "m3508_vel_kd" => (
            "",
            "速度誤差の秒あたり変化に掛けるゲイン（mA・s/rpm）。ミリ秒基準の値からは1/1000に換算します。",
        ),
        "m3508_max_rpm" => (
            "rpm",
            "位置制御が出すモータ回転速度の上限。機体側の角速度ではありません。",
        ),
        "m3508_max_current_ma" => (
            "mA",
            "C620へ出す電流指令の上限。発生トルクと発熱に影響します。",
        ),
        "m3508_max_temperature_c" => ("°C", "この温度を超えたM3508を過熱と判定するしきい値。"),
        "el05_loc_kp" => (
            "",
            "EL05内部の位置制御ゲイン。設定値をモータへ書き込みます。",
        ),
        "el05_limit_spd" => (
            "rad/s",
            "EL05のPP位置モード用速度上限（VEL_MAX）。機体側のmm/sとは別の制限です。",
        ),
        "el05_limit_cur" => (
            "A",
            "EL05内部の電流上限。負荷を保持できる範囲と発熱に影響します。",
        ),
        "dm_p_max" => (
            "rad",
            "DMの位置フレームの符号化レンジ。モータ側設定との一致が必要で、機体可動域ではありません。",
        ),
        "dm_v_max" => (
            "rad/s",
            "DMの速度フレームの符号化レンジ。モータ側設定に合わせてください。",
        ),
        "dm_t_max" => (
            "N·m",
            "DMのトルクフレームの符号化レンジ。モータ側設定に合わせてください。",
        ),
        "dm_pos_vel_limit" => (
            "rad/s",
            "DMの位置・速度モードで送る速度上限。zの機体速度とは別の制限です。",
        ),
        "slot0_min" => (
            "rad",
            "slot 0（EL05）の基板側絶対位置の下限。hostの原点採用では移動しません。",
        ),
        "slot0_max" => (
            "rad",
            "slot 0（EL05）の基板側絶対位置の上限。hostの可動域とは別に働きます。",
        ),
        "slot1_min" => (
            "deg",
            "slot 1（M3508）のモータ累積角度の下限。減速後のアーム角度ではありません。",
        ),
        "slot1_max" => (
            "deg",
            "slot 1（M3508）のモータ累積角度の上限。減速後のアーム角度ではありません。",
        ),
        "slot2_min" => (
            "deg",
            "slot 2（M3508、減速前deg）の基板側絶対位置の下限。hostの原点採用では移動しません。",
        ),
        "slot2_max" => (
            "deg",
            "slot 2（M3508、減速前deg）の基板側絶対位置の上限。hostの可動域とは別に働きます。",
        ),
        "c620_esc_id" | "c620_slot2_esc_id" => (
            "",
            "C620のESC ID（1〜8）。実機のIDと一致させます。変更はSAFE中に反映します。",
        ),
        "dm_can_id" => ("", "DMへの指令送信先CAN ID。モータの設定に合わせます。"),
        "dm_mst_id" => (
            "",
            "DMがフィードバックを返すCAN ID。指令送信先IDとは別です。",
        ),
        "el05_motor_id" => ("", "EL05のモータID。拡張CAN IDの宛先に使います。"),
        "el05_host_id" => ("", "EL05と通信するhost側ID。モータIDとは区別してください。"),
        "m3508_period_ms" => (
            "ms",
            "M3508の制御・送信周期。変更すると制御応答とCAN負荷に影響します。",
        ),
        "dm_period_ms" => ("ms", "DMへの指令送信周期。短くすると通信量が増えます。"),
        "el05_period_ms" => ("ms", "EL05への指令送信周期。短くすると通信量が増えます。"),
        "telemetry_period_ms" => (
            "ms",
            "cctlからhostへ状態を通知する周期。画面更新と接続鮮度の判定に影響します。",
        ),
        "watchdog_ms" => (
            "ms",
            "hostからの通信が途絶えたとき、基板が出力停止するまでの時間。",
        ),
        "feedback_timeout_ms" => (
            "ms",
            "モータからの応答を失ったと判定するまでの時間。対象slotを無効化し、原点の再確認が必要になります。",
        ),
        "min_pulse_us" => (
            "µs",
            "PWMサーボに許可するパルス幅の下限。機構に無理のない範囲に設定します。",
        ),
        "max_pulse_us" => (
            "µs",
            "PWMサーボに許可するパルス幅の上限。角度との対応はサーボごとに確認してください。",
        ),
        "max_duty" => ("‰", "DCモータ出力の絶対Duty上限。1000‰が100%に相当します。"),
        "ramp_interval_ms" => (
            "ms",
            "DCモータのDutyを1段階変化させる間隔。大きいほど出力変化が緩やかになります。",
        ),
        "ramp_step" => (
            "‰",
            "DCモータのDutyを1段階で変化させる量。大きいほど加減速が急になります。",
        ),
        "reverse_brake_ms" => (
            "ms",
            "DCモータの回転方向を反転する前に、出力ゼロを保つ時間。",
        ),
        "pwm_frequency_hz" => (
            "Hz",
            "DCモータ駆動のPWM周波数。ドライバとモータの対応範囲で設定します。",
        ),
        "servo_baud" => (
            "baud",
            "STS3215バスの通信速度。接続するサーボ側の設定に合わせます。",
        ),
        "servo_timeout_ms" => (
            "ms",
            "STS3215からの応答を待つ時間。応答が来なければタイムアウトになります。",
        ),
        "wait_for_write_status" => (
            "0/1",
            "STS3215への書き込み後に応答を待つか。0は待たない、1は待つ設定です。",
        ),
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn every_exposed_board_parameter_has_help() {
        let tables: [&[&str]; 4] = [
            &crate::machine::PARAMETER_NAMES,
            &crate::protocol::svmd::PARAMETER_NAMES,
            &crate::protocol::dcmd::PARAMETER_NAMES,
            &crate::protocol::serial_svmd::PARAMETER_NAMES,
        ];
        for name in tables.into_iter().flatten() {
            assert!(super::help(name).is_some(), "説明がありません: {name}");
        }
    }
}
