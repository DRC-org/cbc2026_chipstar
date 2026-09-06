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

  // プロトコル状態とエラーカウンタ。実機でのCAN診断に使う。
  uint32_t protocolStatus() const { return hcan_->Instance->PSR; }
  uint32_t errorCounters() const { return hcan_->Instance->ECR; }

  // バスオフか。ACKの返らない送信が続くと入り、放置すると送受信とも止まったままになる。
  bool busOff() const { return (hcan_->Instance->PSR & FDCAN_PSR_BO) != 0; }

  // バスオフから復帰させる。競技中にバスが乱れただけで死んだままになるのを避ける。
  //
  // バスオフに入ってもHALのStateはBUSYのままなので、HAL_FDCAN_Start()は
  // HAL_ERRORを返して何もしない。復帰にはCCCR.INITを直接落とす必要がある。
  // INITを落とすと、11連続レセシブビットを129回観測する復帰シーケンスが始まる。
  void recover() {
    if (busOff()) CLEAR_BIT(hcan_->Instance->CCCR, FDCAN_CCCR_INIT);
  }

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
