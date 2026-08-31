#include "main.h"
#include "servo_config.hpp"
#include "sts3215.hpp"

#include <cstdint>

extern "C" {
extern UART_HandleTypeDef huart1;  // サーボ用（絶縁・半二重自動切替回路経由）
}

// デバッガの Watch / Live Expressions から状態を確認するための変数。
// 正常時は g_servo_status が 0 (Result::Ok) のままになる。
volatile Sts3215::Result g_servo_status = Sts3215::Result::Timeout;
volatile uint16_t g_servo_position = 0;
volatile uint8_t g_servo_error_flags = 0;

namespace {
Sts3215 servo(&huart1, config::servo::TIMEOUT_MS, config::servo::WAIT_FOR_WRITE_STATUS);

uint16_t sweep_position = config::servo::SWEEP_MIN_POSITION;
uint32_t last_command_ms = 0;
uint32_t last_feedback_ms = 0;

Sts3215::Target makeTarget(uint16_t position) {
  return Sts3215::Target{config::servo::ID, config::servo::ACCELERATION, position, 0,
                         config::servo::SPEED};
}
}  // namespace

extern "C" void setup(void) {
  HAL_Delay(config::servo::BOOT_DELAY_MS);

  g_servo_status = servo.ping(config::servo::ID);
  g_servo_error_flags = servo.lastServoError();
  if (g_servo_status == Sts3215::Result::Ok) {
    g_servo_status = servo.setTorque(config::servo::ID, true);
  }
  if (g_servo_status == Sts3215::Result::Ok) {
    g_servo_status = servo.setTarget(makeTarget(config::servo::CENTER_POSITION));
  }

  last_command_ms = HAL_GetTick();
  last_feedback_ms = last_command_ms;
}

extern "C" void loop(void) {
  const uint32_t now = HAL_GetTick();

  // 可動域の両端を往復させる。
  if (now - last_command_ms >= config::period::SERVO_COMMAND_MS) {
    last_command_ms = now;
    g_servo_status = servo.setTarget(makeTarget(sweep_position));
    g_servo_error_flags = servo.lastServoError();
    sweep_position = (sweep_position == config::servo::SWEEP_MIN_POSITION)
                         ? config::servo::SWEEP_MAX_POSITION
                         : config::servo::SWEEP_MIN_POSITION;
  }

  if (now - last_feedback_ms >= config::period::SERVO_FEEDBACK_MS) {
    last_feedback_ms = now;

    uint16_t position = 0;
    g_servo_status = servo.readPosition(config::servo::ID, position);
    g_servo_error_flags = servo.lastServoError();
    if (g_servo_status == Sts3215::Result::Ok) {
      g_servo_position = position;
    }
  }
}
