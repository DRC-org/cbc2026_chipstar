#pragma once

#include "c620_group.hpp"
#include "can_bus.hpp"
#include "dm_motor.hpp"
#include "domain/can_frame.hpp"
#include "domain/run_state.hpp"
#include "domain/feedback_watch.hpp"
#include "domain/parameters.hpp"
#include "domain/jog.hpp"
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
  bool setJog(uint8_t slot, float velocity);

  // 実行時パラメータ。書き込みなしで実機調整を終えるための入口。
  // 通信IDの差し替えはSAFE中だけ受理する。
  bool setParameter(uint8_t id, float value);

  // モータ側の設定を入れ直す。モータが電源を入れ直すと制御モードなどを
  // 失うが、cctlは起動時にしか設定していないため復帰できない。SAFE中のみ。
  bool reinitialize(uint8_t slots);

  // 保存済みパラメータを消して既定値へ戻す。SAFE中のみ。
  bool resetParameters();

  // 変更されたパラメータを不揮発へ書き戻す。毎周期呼んでよい。
  void flushParameters();

  // DMドライバのレジスタ。SAFE中だけ受理する。
  bool readDmRegister(uint8_t rid);
  bool writeDmRegister(uint8_t rid, uint32_t raw);
  bool takeDmRegisterReply(uint8_t& rid, uint32_t& raw);
  const domain::Parameters& parameters() const { return parameters_; }
  float target(uint8_t slot) const;
  float measured(uint8_t slot) const;
  // slot単位の異常。bit7はフィードバック途絶を表す。
  uint8_t errorBits(uint8_t slot) const;
  // フィードバックが途絶えたslotのbit mask。
  uint8_t staleSlots() const { return feedback_.stale(); }

 private:
  void applySlotStates();
  bool slotActive(uint8_t bit) const;

  void applyParameter(uint8_t id);
  void applyAllParameters();
  void initMotor(uint8_t slot);
  void checkFeedback(uint32_t now);

  domain::Parameters parameters_;
  uint32_t param_dirty_ms_ = 0;
  bool param_dirty_ = false;
  El05Motor slot0_;
  M3508Motor slot1_;
  DmMotor slot2_;
  C620Group c620_group_;
  float targets_[domain::SLOT_COUNT] = {};
  domain::Jog jog_[domain::SLOT_COUNT];
  uint32_t last_jog_ms_ = 0;
  domain::RunMode mode_ = domain::RunMode::Safe;
  uint8_t enabled_slots_ = 0;
  domain::FeedbackWatch feedback_;
  uint32_t last_m3508_ms_ = 0;
  uint32_t last_dm_ms_ = 0;
  uint32_t last_el05_ms_ = 0;
};
