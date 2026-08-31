#pragma once

#include "domain/can_frame.hpp"
#include "main.h"

#include <cstdint>

// HAL FDCAN の薄いラッパ。1 本のバスを標準/拡張フレームで送受信する。
// 受信は RX FIFO0 のポーリング方式（全ID受理）。
class CanBus {
 public:
  explicit CanBus(FDCAN_HandleTypeDef* hcan) : hcan_(hcan) {}

  // グローバルフィルタを FIFO0 受理に設定し、バスを開始する。
  bool begin();

  // 標準ID(11bit) データフレーム送信（最大8byte）。
  bool sendStd(uint16_t id, const uint8_t* data, uint8_t len);
  // 拡張ID(29bit) データフレーム送信（最大8byte）。
  bool sendExt(uint32_t id, const uint8_t* data, uint8_t len);

  // FIFO0 に受信があれば 1 フレーム取り出して true を返す。
  bool receive(domain::CanFrame& frame);

 private:
  bool send(uint32_t id, uint32_t id_type, const uint8_t* data, uint8_t len);

  FDCAN_HandleTypeDef* hcan_;
};
