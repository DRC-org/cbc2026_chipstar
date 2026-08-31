#include "c620_group.hpp"

bool C620Group::add(M3508Motor& motor) {
  if (count_ >= domain::c620::MOTORS_PER_FRAME) {
    return false;
  }
  if (domain::c620::groupCommandId(motor.escId()) != frame_.commandId()) {
    return false;
  }

  motors_[count_++] = &motor;
  return true;
}

bool C620Group::send() {
  // 書かれなかったスロットを 0 にしてから積む。
  frame_.clear();
  for (std::size_t i = 0; i < count_; ++i) {
    frame_.set(motors_[i]->escId(), motors_[i]->commandRaw());
  }

  return bus_.sendStd(frame_.commandId(), frame_.data(), 8);
}
