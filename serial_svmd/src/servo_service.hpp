#pragma once
#include "sts3215.hpp"

// CAN拡張操作。応答は要求op/id/tagと照合する。UARTの実行結果を返す。
class ServoService {
 public:
  using Emit = void (*)(uint16_t, const uint8_t*);
  using Baud = bool (*)(uint32_t);
  ServoService(Sts3215& bus, Emit emit, Baud baud) : bus_(bus), emit_(emit), baud_(baud) {}
  void handle(const uint8_t* packet, bool running);
  bool stop();
  bool active() const { return active_; }
 private:
  Sts3215::Result configure(uint8_t id, uint8_t reg, uint16_t value, uint8_t width);
  Sts3215::Result execute();
  Sts3215& bus_;
  Emit emit_;
  Baud baud_;
  Sts3215::Target staged_[16]{};
  uint8_t count_ = 0;
  uint8_t ids_[16]{};
  uint8_t used_ = 0;
  bool active_ = false;
  bool stop_pending_ = false;
  bool executed_ = false;
  uint8_t last_tag_ = 0;
  uint8_t last_result_ = 0;
};
