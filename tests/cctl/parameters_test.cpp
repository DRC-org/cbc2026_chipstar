#include "doctest.h"

#include "device_config.hpp"
#include "domain/parameters.hpp"

using domain::ParamId;
using domain::Parameters;

namespace {
uint8_t id(ParamId value) {
    return static_cast<uint8_t>(value);
}
}  // namespace

TEST_CASE("既定値はdevice_configと一致する") {
    const Parameters parameters;
    CHECK(parameters.get(ParamId::M3508PosKp) == doctest::Approx(config::m3508::POS_KP));
    CHECK(parameters.get(ParamId::El05LocKp) == doctest::Approx(config::el05::LOC_KP));
    CHECK(parameters.get(ParamId::DmPMax) == doctest::Approx(config::dm::P_MAX));
    CHECK(parameters.getMs(ParamId::WatchdogMs) == config::period::WATCHDOG_MS);
    CHECK(parameters.getU8(ParamId::C620EscId) == config::can_id::C620_ESC_ID);
}

TEST_CASE("値を書き換えて読み戻せる") {
    Parameters parameters;
    CHECK(parameters.set(id(ParamId::M3508VelKp), 1.25f));
    CHECK(parameters.get(ParamId::M3508VelKp) == doctest::Approx(1.25f));

    parameters.reset();
    CHECK(parameters.get(ParamId::M3508VelKp) == doctest::Approx(config::m3508::VEL_KP));
}

TEST_CASE("FWが壊れる値だけを拒否する") {
    Parameters parameters;
    // 制御周期0は待ち時間の判定を壊す。
    CHECK_FALSE(parameters.set(id(ParamId::M3508PeriodMs), 0.0f));
    // CAN IDは標準IDの範囲を超えられない。
    CHECK_FALSE(parameters.set(id(ParamId::DmCanId), 2048.0f));
    CHECK_FALSE(parameters.set(id(ParamId::C620EscId), 0.0f));
    CHECK_FALSE(parameters.set(id(ParamId::C620EscId), 9.0f));
    // DMの符号化レンジは0だと除算が壊れる。
    CHECK_FALSE(parameters.set(id(ParamId::DmPMax), 0.0f));
    // 有限でない値は受け付けない。
    CHECK_FALSE(parameters.set(id(ParamId::M3508PosKp), 1.0f / 0.0f));
    // 未定義のidも拒否する。
    CHECK_FALSE(parameters.set(domain::PARAM_COUNT, 1.0f));
}

TEST_CASE("強いゲインと高い上限は運用側の判断に任せる") {
    Parameters parameters;
    // 焼き直せない前提では、保守的な上限のほうが詰みの原因になる。
    CHECK(parameters.set(id(ParamId::M3508PosKp), 500.0f));
    CHECK(parameters.set(id(ParamId::M3508MaxCurrentMa), 16000.0f));
    CHECK(parameters.set(id(ParamId::El05LimitCur), 11.0f));
    // 可動域は負にも広げられる。
    CHECK(parameters.set(id(ParamId::Slot1Min), -100000.0f));
}

TEST_CASE("通信IDの変更だけSAFEを要求する") {
    CHECK(domain::requiresSafe(id(ParamId::DmCanId)));
    CHECK(domain::requiresSafe(id(ParamId::C620EscId)));
    CHECK(domain::requiresSafe(id(ParamId::El05HostId)));
    CHECK_FALSE(domain::requiresSafe(id(ParamId::M3508PosKp)));
    CHECK_FALSE(domain::requiresSafe(id(ParamId::WatchdogMs)));
}
