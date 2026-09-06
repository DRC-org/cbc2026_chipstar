#pragma once

#include "c620_group.hpp"
#include "can_bus.hpp"
#include "dm_motor.hpp"
#include "domain/can_frame.hpp"
#include "domain/run_state.hpp"
#include "domain/parameters.hpp"
#include "el05_motor.hpp"
#include "m3508_motor.hpp"

#include <cstdint>

// cctlに物理接続されたアクチュエータを、機体上の意味を持たないslotとして扱う。
class ActuatorController {
 public:
  explicit ActuatorController(CanBus& bus);

  void begin();
  void dispatchRx(const domain::CanFrame& frame);
  void update();

  void setMode(domain::RunMode mode);
  domain::RunMode mode() const { return mode_; }
  void setSlotsEnabled(uint8_t slots, bool enabled);
  uint8_t enabledSlots() const { return enabled_slots_; }
  void home(uint8_t slots);

  bool setTarget(uint8_t slot, float value);

  // 実行時パラメータ。書き込みなしで実機調整を終えるための入口。
  // 通信IDの差し替えはSAFE中だけ受理する。
  bool setParameter(uint8_t id, float value);
  const domain::Parameters& parameters() const { return parameters_; }
  float target(uint8_t slot) const;
  float measured(uint8_t slot) const;
  uint8_t errorBits() const;
  // フィードバックが途絶えたslotのbit mask。
  uint8_t staleSlots() const { return stale_slots_; }

 private:
  void applySlotStates();
  bool slotActive(uint8_t bit) const;

  void applyParameter(uint8_t id);
  void checkFeedback(uint32_t now);

  domain::Parameters parameters_;
  El05Motor slot0_;
  M3508Motor slot1_;
  DmMotor slot2_;
  C620Group c620_group_;
  float targets_[domain::SLOT_COUNT] = {};
  domain::RunMode mode_ = domain::RunMode::Safe;
  uint8_t enabled_slots_ = 0;
  uint32_t last_rx_ms_[domain::SLOT_COUNT] = {};
  uint8_t stale_slots_ = 0;
  uint32_t last_m3508_ms_ = 0;
  uint32_t last_dm_ms_ = 0;
  uint32_t last_el05_ms_ = 0;
};
