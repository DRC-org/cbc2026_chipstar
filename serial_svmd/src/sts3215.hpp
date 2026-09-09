#pragma once

#include "domain/sts3215_protocol.hpp"
#include "main.h"

#include <cstddef>
#include <cstdint>

// Feetech STS3215 シリアルバスサーボのドライバ。
// パケットの組立・検証は domain::sts3215 が担い、本クラスは UART との
// やり取りとタイムアウト管理を受け持つ。
//
// 基板上の絶縁回路が半二重の送受信方向を自動で切り替えるため、
// MCU 側で方向制御 GPIO を操作する必要はない。
// 送信はblocking API、受信は循環DMAで行う。
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
    UnsupportedMode,
    ReadbackMismatch,
  };

  using Target = domain::sts3215::Target;

  static constexpr uint8_t BROADCAST_ID = domain::sts3215::BROADCAST_ID;
  static constexpr uint16_t MAX_POSITION = domain::sts3215::MAX_POSITION;
  static constexpr uint8_t BAUD_CODE_115200 = domain::sts3215::BAUD_CODE_115200;
  static constexpr std::size_t MAX_SYNC_TARGETS = domain::sts3215::MAX_SYNC_TARGETS;

  // wait_for_write_status: 書き込み命令のステータス応答を待つか。
  // falseではSYNC_WRITEを使用。トルク・目標の成否は読戻しで確認する。
  Sts3215(UART_HandleTypeDef* huart, uint32_t timeout_ms, bool wait_for_write_status)
      : huart_(huart),
        timeout_ms_(timeout_ms),
        wait_for_write_status_(wait_for_write_status) {}

  // hostからの実行時調整。サーボの応答が遅い個体に合わせられる。
  void setTiming(uint32_t timeout_ms, bool wait_for_write_status) {
    timeout_ms_ = timeout_ms;
    wait_for_write_status_ = wait_for_write_status;
  }

  // USART1 RXを循環DMAで受ける。huart->hdmarxは基板側で初期化する。
  Result startReceiver();
  void stopReceiver();
  Result preparePosition(uint8_t id, const Target* target = nullptr);

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
  Result writeVerified(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length);
  Result verifyReadback(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length);
  struct ReadbackMismatch {
    uint8_t address = 0;
    uint8_t expected = 0;
    uint8_t actual = 0;
  };
  ReadbackMismatch lastReadbackMismatch() const { return mismatch_; }
  Result writeUnacknowledged(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length);
  Result syncWriteRawTargets(const Target* targets, std::size_t count);

  // 直近の応答パケットが返したサーボ側のエラービット。
  uint8_t lastServoError() const { return last_servo_error_; }

  // 直近の HAL 呼び出しの結果。通信不良の切り分けに使う。
  HAL_StatusTypeDef lastHalStatus() const { return last_hal_status_; }

 private:
  Result writeAbsoluteVerified(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length);
  Result readOnce(uint8_t id, uint8_t address, uint8_t* data, uint8_t length);
  Result sendInstruction(uint8_t id, uint8_t instruction, const uint8_t* parameters,
                         uint8_t parameter_count);
  Result receiveStatus(uint8_t expected_id, uint8_t* parameters, uint8_t capacity,
                       uint8_t& parameter_count);
  Result receiveExact(uint8_t* data, uint16_t length, uint32_t start_ms);
  void flushRx();
  static constexpr uint16_t RX_BUFFER_SIZE = 256;
  uint8_t rx_buffer_[RX_BUFFER_SIZE] = {};
  uint16_t rx_tail_ = 0;
  bool dma_rx_ = false;
  bool rx_restart_pending_ = false;

  UART_HandleTypeDef* huart_;
  uint32_t timeout_ms_;
  bool wait_for_write_status_;
  HAL_StatusTypeDef last_hal_status_ = HAL_OK;
  uint8_t last_servo_error_ = 0;
  ReadbackMismatch mismatch_;
};
