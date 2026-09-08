#pragma once

#include <cstddef>
#include <cstdint>

namespace domain {

// hostが実行時に変更できる基板パラメータ。
//
// FWを書き直さずに実機調整を終えられるようにするため、調整対象になる値は
// すべてここへ集める。保存はparam_storeがSAFE中にまとめて行い、
// hostは接続時に機体設定を適用して読戻しを確認する。
enum class ParamId : uint8_t {
    // M3508 + C620 のカスケードPID
    M3508PosKp = 0,
    M3508PosKi,
    M3508PosKd,
    M3508MaxRpm,
    M3508VelKp,
    M3508VelKi,
    M3508VelKd,
    M3508MaxCurrentMa,
    // EL05 のドライバ内パラメータ。設定時にモータへ書き込む。
    El05LocKp,
    El05LimitSpd,
    El05LimitCur,
    // DM のフレーム符号化レンジ。モータ側の設定と一致させる必要がある。
    DmPMax,
    DmVMax,
    DmTMax,
    DmPosVelLimit,
    // 廃止済みの基板側可動域。保存形式と後続IDの互換性のため欠番を維持する。
    Reserved15,
    Reserved16,
    Reserved17,
    Reserved18,
    Reserved19,
    Reserved20,
    // モータのCAN ID。変更はSAFE中だけ受理する。
    C620EscId,
    DmCanId,
    DmMstId,
    El05MotorId,
    El05HostId,
    // 制御周期と通信期限 [ms]
    M3508PeriodMs,
    DmPeriodMs,
    El05PeriodMs,
    TelemetryPeriodMs,
    WatchdogMs,
    // モータのフィードバックが途絶えたと判断するまでの時間 [ms]
    FeedbackTimeoutMs,
    // M3508の過熱と判断する温度 [degC]
    M3508MaxTemperatureC,
    // 2台目のM3508。旧DMのIDは予約として維持する。
    M3508Slot2PosKp,
    M3508Slot2PosKi,
    M3508Slot2PosKd,
    M3508Slot2MaxRpm,
    M3508Slot2VelKp,
    M3508Slot2VelKi,
    M3508Slot2VelKd,
    M3508Slot2MaxCurrentMa,
    C620Slot2EscId,
    M3508Slot2MaxTemperatureC,
    Count,
};

constexpr std::size_t PARAM_COUNT = static_cast<std::size_t>(ParamId::Count);

// idの変更にSAFEを要するか。通信IDを走行中に差し替えると、
// 指令の宛先とフィードバックの解釈が食い違う。
bool requiresSafe(uint8_t id);

class Parameters {
 public:
    Parameters() { reset(); }

    // device_config.hpp の既定値へ戻す。
    void reset();

    // 範囲外なら false を返し、値を変更しない。
    bool set(uint8_t id, float value);
    bool restore(const float* values, std::size_t count);

    float get(ParamId id) const { return values_[static_cast<std::size_t>(id)]; }
    float get(uint8_t id) const { return values_[id]; }
    uint32_t getMs(ParamId id) const { return static_cast<uint32_t>(get(id)); }
    uint8_t getU8(ParamId id) const { return static_cast<uint8_t>(get(id)); }
    uint16_t getU16(ParamId id) const { return static_cast<uint16_t>(get(id)); }

    // FWが壊れる値だけを弾く。強いゲインや高い電流上限は運用側の判断に任せる。
    static bool valid(uint8_t id, float value);

 private:
    float values_[PARAM_COUNT] = {};
};

}  // namespace domain
