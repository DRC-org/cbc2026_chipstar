//! 基板の公開パラメータの説明。ID・意味はdevice_protocol.mdとFW定義に対応する。
pub(super) fn label(name: &str) -> Option<&'static str> {
    Some(match name {
        "m3508_pos_kp" => "θ 位置Pゲイン",
        "m3508_pos_ki" => "θ 位置Iゲイン",
        "m3508_pos_kd" => "θ 位置Dゲイン",
        "m3508_vel_kp" => "θ 速度Pゲイン",
        "m3508_vel_ki" => "θ 速度Iゲイン",
        "m3508_vel_kd" => "θ 速度Dゲイン",
        "m3508_max_rpm" => "θ モータ速度上限",
        "m3508_max_current_ma" => "θ 電流指令上限",
        "m3508_max_temperature_c" => "θ 過熱判定温度",
        "m3508_slot2_pos_kp" => "z 位置Pゲイン",
        "m3508_slot2_pos_ki" => "z 位置Iゲイン",
        "m3508_slot2_pos_kd" => "z 位置Dゲイン",
        "m3508_slot2_vel_kp" => "z 速度Pゲイン",
        "m3508_slot2_vel_ki" => "z 速度Iゲイン",
        "m3508_slot2_vel_kd" => "z 速度Dゲイン",
        "m3508_slot2_max_rpm" => "z モータ速度上限",
        "m3508_slot2_max_current_ma" => "z 電流指令上限",
        "m3508_slot2_max_temperature_c" => "z 過熱判定温度",
        "el05_loc_kp" => "r モータ内部位置Pゲイン",
        "el05_limit_spd" => "r モータ内部速度上限",
        "el05_limit_cur" => "r モータ内部電流上限",
        "c620_esc_id" => "θ C620 ID",
        "c620_slot2_esc_id" => "z C620 ID",
        "el05_motor_id" => "r モータID",
        "el05_host_id" => "r host ID",
        "m3508_period_ms" => "M3508制御周期",
        "el05_period_ms" => "EL05指令周期",
        "telemetry_period_ms" => "cctl状態通知周期",
        "watchdog_ms" => "通信監視時間",
        "feedback_timeout_ms" => "モータ応答待ち時間",
        "dm_p_max" => "DM位置の符号化範囲",
        "dm_v_max" => "DM速度の符号化範囲",
        "dm_t_max" => "DMトルクの符号化範囲",
        "dm_pos_vel_limit" => "DM位置指令の速度上限",
        "dm_can_id" => "DM指令先CAN ID",
        "dm_mst_id" => "DM応答先CAN ID",
        "dm_period_ms" => "DM指令周期",
        "min_pulse_us" => "PWMパルス幅の下限",
        "max_pulse_us" => "PWMパルス幅の上限",
        "max_duty" => "DCモータ出力上限",
        "ramp_interval_ms" => "DCモータ出力の更新間隔",
        "ramp_step" => "DCモータ出力の変化量",
        "reverse_brake_ms" => "DCモータ反転前の停止時間",
        "pwm_frequency_hz" => "DCモータPWM周波数",
        "servo_baud" => "STS3215通信速度",
        "servo_timeout_ms" => "STS3215応答待ち時間",
        "wait_for_write_status" => "STS3215書込み応答の確認方法",
        _ => return None,
    })
}

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
            "軸の最高速度と換算係数から自動生成されるCCTL内部値です。",
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
            "軸の最高速度と換算係数から自動生成されるEL05 CSP速度上限です。",
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
            "STS3215バスの通信速度。C018の出荷時は1000000。サーボ本体の保存設定は変更しないので、全個体と同じ値にします。",
        ),
        "servo_timeout_ms" => (
            "ms",
            "STS3215からの応答を待つ時間。応答が来なければタイムアウトになります。",
        ),
        "wait_for_write_status" => (
            "0/1",
            "0は応答を返さないSYNC_WRITE、1は通常WRITEの応答も待ちます。どちらもトルク・目標値を読み戻して確認します。通常は0。",
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
        for name in tables
            .into_iter()
            .flatten()
            .filter(|name| !name.starts_with("reserved_"))
        {
            assert!(super::help(name).is_some(), "説明がありません: {name}");
            assert!(super::label(name).is_some(), "表示名がありません: {name}");
        }
    }
}
