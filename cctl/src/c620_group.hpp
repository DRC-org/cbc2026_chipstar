#pragma once

#include "can_bus.hpp"
#include "domain/c620_codec.hpp"
#include "m3508_motor.hpp"

#include <cstddef>

// 同一グループの C620 をまとめて送信する。
//
// C620 の指令フレームは 1 本で 4 台分を運ぶ。モータが個別にフレームを送ると、
// 後から届いたフレームが他の台のスロットを 0 で上書きしてしまうため、
// 送信はここに集約する。
class C620Group {
 public:
  // command_id: domain::c620::COMMAND_ID_1_TO_4 または COMMAND_ID_5_TO_8。
  C620Group(CanBus& bus, uint16_t command_id) : bus_(bus), frame_(command_id) {}

  // 送信対象に加える。ESC ID がこのグループ外、または満杯なら false。
  bool add(M3508Motor& motor);

  // 全メンバの指令を 1 フレームにまとめて送る。
  bool send();

  std::size_t size() const { return count_; }

 private:
  CanBus& bus_;
  domain::c620::CommandFrame frame_;
  M3508Motor* motors_[domain::c620::MOTORS_PER_FRAME] = {};
  std::size_t count_ = 0;
};
