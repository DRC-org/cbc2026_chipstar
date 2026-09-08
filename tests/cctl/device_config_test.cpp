#include "doctest.h"

#include "device_config.hpp"

TEST_CASE("制御周期とWatchdogは有効") {
    CHECK(config::period::M3508_MS > 0);
    CHECK(config::period::DM_MS > 0);
    CHECK(config::period::EL05_MS > 0);
    CHECK(config::period::WATCHDOG_MS > config::period::EL05_MS);
}

TEST_CASE("CAN標準IDは衝突しない") {
    constexpr uint16_t dm_command = 0x100 + config::can_id::DM_CAN_ID;
    CHECK(config::can_id::C620_FEEDBACK ==
          config::can_id::C620_COMMAND + config::can_id::C620_ESC_ID);
    CHECK(dm_command != config::can_id::C620_COMMAND);
    CHECK(dm_command != config::can_id::C620_FEEDBACK);
    CHECK(config::can_id::DM_MST_ID != config::can_id::C620_COMMAND);
    CHECK(config::can_id::DM_MST_ID != config::can_id::C620_FEEDBACK);
}
