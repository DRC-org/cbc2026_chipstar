#pragma once

#include <cstddef>
#include <cstdint>

namespace config {
constexpr uint8_t PROTOCOL_VERSION = 1;
constexpr std::size_t MAX_SERVOS = 16;
constexpr uint32_t SERVO_TIMEOUT_MS = 20;
constexpr bool WAIT_FOR_WRITE_STATUS = false;
constexpr uint16_t MAX_SPEED = 1000;
constexpr uint8_t MAX_ACCELERATION = 254;
constexpr uint32_t WATCHDOG_MS = 250;
constexpr std::size_t LINE_CAPACITY = 96;
}  // namespace config
