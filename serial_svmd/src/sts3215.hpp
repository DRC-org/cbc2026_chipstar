#pragma once

#include "main.h"

#include <cstddef>
#include <cstdint>

// STS プロトコルのレジスタアドレス。
namespace sts3215_reg {
constexpr uint8_t ID = 5;
constexpr uint8_t BAUD_RATE = 6;
constexpr uint8_t TORQUE_ENABLE = 40;
constexpr uint8_t ACCELERATION = 41;
constexpr uint8_t GOAL_POSITION = 42;
constexpr uint8_t GOAL_TIME = 44;
constexpr uint8_t GOAL_SPEED = 46;
constexpr uint8_t PRESENT_POSITION = 56;
}  // namespace sts3215_reg

// Feetech STS3215 シリアルバスサーボのプロトコルドライバ。
// 基板上の絶縁回路が半二重の送受信方向を自動で切り替えるため、
// MCU 側で方向制御 GPIO を操作する必要はない。
// 通信は HAL の Blocking API によるポーリングで行う。
class Sts3215 {
 public:
  enum class Result {
    Ok = 0,
    ArgumentError,
    HalError,
    Timeout,
    ProtocolError,
    ChecksumError,
    ServoError,
  };

  // 加速度・目標位置・目標時間・目標速度をまとめた指令値。
  // アドレス41から連続する4レジスタに対応する。
  struct Target {
    uint8_t id;
    uint8_t acceleration;
    uint16_t position;
    uint16_t time;
    uint16_t speed;
  };

  static constexpr uint8_t BROADCAST_ID = 0xFE;
  static constexpr uint16_t MAX_POSITION = 4095;
  static constexpr uint8_t BAUD_CODE_115200 = 4;
  // syncWriteTargets() で一度に指定できるサーボ数。
  static constexpr std::size_t MAX_SYNC_TARGETS = 16;

  // wait_for_write_status: 書き込み命令のステータス応答を待つか。
  // false にすると指令の往復待ちがなくなる代わりに、書き込みの成否は検出できない。
  Sts3215(UART_HandleTypeDef* huart, uint32_t timeout_ms, bool wait_for_write_status)
      : huart_(huart),
        timeout_ms_(timeout_ms),
        wait_for_write_status_(wait_for_write_status) {}

  // 指定 ID の応答を確認する。
  Result ping(uint8_t id);

  // アドレス address から length バイト読み出す。
  Result read(uint8_t id, uint8_t address, uint8_t* data, uint8_t length);

  // アドレス address へ length バイト書き込む。
  Result write(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length);

  Result setTorque(uint8_t id, bool enable);

  // 加速度から目標速度までを1パケットで書き込む。
  Result setTarget(const Target& target);

  // 複数サーボの目標値をブロードキャストで一括送信する（応答なし）。
  Result syncWriteTargets(const Target* targets, std::size_t count);

  Result readPosition(uint8_t id, uint16_t& position);

  // 直近の応答パケットが返したサーボ側のエラービット。
  uint8_t lastServoError() const { return last_servo_error_; }

  // 直近の HAL 呼び出しの結果。通信不良の切り分けに使う。
  HAL_StatusTypeDef lastHalStatus() const { return last_hal_status_; }

 private:
  Result sendInstruction(uint8_t id, uint8_t instruction, const uint8_t* parameters,
                         uint8_t parameter_count);
  Result receiveStatus(uint8_t expected_id, uint8_t* parameters, uint8_t capacity,
                       uint8_t& parameter_count);
  Result receiveExact(uint8_t* data, uint16_t length, uint32_t start_ms);
  void flushRx();

  UART_HandleTypeDef* huart_;
  uint32_t timeout_ms_;
  bool wait_for_write_status_;
  HAL_StatusTypeDef last_hal_status_ = HAL_OK;
  uint8_t last_servo_error_ = 0;
};
