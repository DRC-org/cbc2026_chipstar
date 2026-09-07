#include "servo_service.hpp"
#include <cstring>

namespace {
using R = Sts3215::Result;
constexpr uint32_t BAUDS[] = {1000000, 500000, 250000, 128000, 115200, 76800, 57600, 38400};
uint16_t be16(const uint8_t* p) { return static_cast<uint16_t>(p[0]) << 8 | p[1]; }
uint16_t le16(const uint8_t* p) { return p[0] | static_cast<uint16_t>(p[1]) << 8; }
bool writable(uint8_t reg, uint16_t value, uint8_t width) {
  if (width == 1) {
    if (reg == 5) return value >= 1 && value <= 253;
    if (reg == 6) return value < 8;
    if (reg == 33) return value == 0 || value == 1 || value == 3;
    if (reg == 26 || reg == 27) return value <= 255;
    if (reg >= 21 && reg <= 23) return value <= 254;
    if (reg == 41) return value <= 254;
  }
  return width == 2 && (reg == 9 || reg == 11 || reg == 31) && value <= 4095;
}
}

bool ServoService::stop() {
  bool ok = true;
  for (uint8_t i = 0; i < used_; ++i) {
    if (bus_.setTorque(ids_[i], false) != R::Ok) ok = false;
  }
  active_ = false;
  count_ = 0;
  stop_pending_ = !ok;
  if (ok) used_ = 0;
  return ok;
}

R ServoService::configure(uint8_t id, uint8_t reg, uint16_t value, uint8_t width) {
  const uint8_t original_id = id;
  if (!writable(reg, value, width)) return R::ArgumentError;
  auto result = bus_.setTorque(id, false);
  if (result != R::Ok) return result;
  uint8_t unlock = 0;
  if (reg < 40) {
    result = bus_.writeVerified(id, 55, &unlock, 1);
    if (result != R::Ok) return result;
  }
  const uint8_t data[] = {static_cast<uint8_t>(value), static_cast<uint8_t>(value >> 8)};
  if (reg == 5 || reg == 6) {
    // 旧ID・旧baudのWRITE ACKを待たず、新設定で読戻す。
    // write()はwait設定でACK待ちを行うため、専用の無応答書込みを使用。
    result = bus_.writeUnacknowledged(id, reg, data, 1);
    if (result != R::Ok) return result;
    HAL_Delay(20);
    if (reg == 5) id = data[0];
    if (reg == 6 && !baud_(BAUDS[value])) return R::HalError;
    uint8_t actual = 0;
    result = bus_.read(id, reg, &actual, 1);
    if (result == R::Ok && actual != data[0]) result = R::ReadbackMismatch;
  } else {
    if (reg == 33 && value == 3) {
      const uint8_t zeros[4]{};
      result = bus_.writeVerified(id, 9, zeros, 4);
    }
    if (result == R::Ok) result = bus_.writeVerified(id, reg, data, width);
  }
  const uint8_t lock = 1;
  const auto locked = reg < 40 ? bus_.writeVerified(id, 55, &lock, 1) : R::Ok;
  if (reg == 5 && result == R::Ok && locked == R::Ok) {
    // 停止確認先も新IDへ移す。設定後に旧IDを待ち続けない。
    for (uint8_t i = 0; i < used_; ++i) if (ids_[i] == original_id) ids_[i] = id;
  }
  return result == R::Ok ? locked : result;
}

R ServoService::execute() {
  // 全対象のモード・目標・応答を確認してから出力を有効化する。
  uint8_t modes[16]{};
  uint16_t minimums[16]{}, maximums[16]{};
  for (uint8_t i = 0; i < count_; ++i) {
    const auto& t = staged_[i];
    auto r = bus_.read(t.id, 33, &modes[i], 1);
    if (r != R::Ok) return r;
    if (modes[i] != 0 && modes[i] != 1 && modes[i] != 3) return R::UnsupportedMode;
    if ((modes[i] == 1 && t.position != 0) ||
        (t.speed & 0x7fff) > 1000 || (modes[i] != 1 && t.speed > 1000) ||
        (t.position & 0x7fff) > 28672) return R::ArgumentError;
    if (modes[i] == 0 || modes[i] == 3) {
      uint8_t limits[4]{};
      r = bus_.read(t.id, 9, limits, 4);
      if (r != R::Ok) return r;
      const uint16_t minimum = le16(limits), maximum = le16(limits + 2);
      minimums[i] = minimum;
      maximums[i] = maximum;
      if (modes[i] == 3 && (minimum || maximum)) return R::UnsupportedMode;
      if (modes[i] == 0 && (minimum || maximum) &&
          (t.position < minimum || t.position > maximum || t.position > 4095)) return R::ArgumentError;
    }
    bool known = false;
    for (uint8_t j = 0; j < used_; ++j) known |= ids_[j] == t.id;
    if (!known) {
      if (used_ == 16) return R::ArgumentError;
      ids_[used_++] = t.id;
    }
  }
  for (uint8_t i = 0; i < count_; ++i) {
    const auto id = staged_[i].id;
    uint8_t torque = 0;
    auto r = bus_.read(id, 40, &torque, 1);
    if (r != R::Ok) return r;
    if (!torque) {
      Sts3215::Target neutral{id, staged_[i].acceleration, 0, 0, 0};
      if (modes[i] == 0) {
        uint8_t position[2]{};
        r = bus_.read(id, 56, position, 2);
        if (r != R::Ok) return r;
        neutral.position = le16(position);
        if ((neutral.position & 0x7fff) > 28672 ||
            ((minimums[i] || maximums[i]) &&
             (neutral.position < minimums[i] || neutral.position > maximums[i] ||
              neutral.position > 4095))) return R::ArgumentError;
      }
      uint8_t data[7];
      domain::sts3215::encodeTarget(neutral, data);
      r = bus_.writeVerified(id, 41, data, 7);
      if (r != R::Ok) return r;
      r = bus_.setTorque(id, true);
      if (r != R::Ok) return r;
    }
  }
  auto r = bus_.syncWriteRawTargets(staged_, count_);
  if (r != R::Ok) return r;
  active_ = true;
  // 相対ステップも送信は1回だけ。確認はREADのみで、再送しない。
  for (uint8_t i = 0; i < count_; ++i) {
    uint8_t expected[7], actual[7];
    domain::sts3215::encodeTarget(staged_[i], expected);
    r = bus_.read(staged_[i].id, 41, actual, 7);
    if (r != R::Ok) return r;
    if (std::memcmp(expected, actual, 7)) return R::ReadbackMismatch;
  }
  return R::Ok;
}

void ServoService::handle(const uint8_t* p, bool running) {
  const uint8_t op = p[1], id = p[2], tag = p[3];
  uint8_t reply[8] = {1, op, id, tag, 0, 0, 0, 0};
  R result = R::ArgumentError;
  if (p[0] != 1) { reply[4] = 1; emit_(0x326, reply); return; }
  if (op == 23) {
    if (executed_ && tag == last_tag_ && count_ == 0) {
      reply[4] = last_result_;
      emit_(0x326, reply);
      return;
    }
    if (running && !stop_pending_ && count_ && count_ == id) {
      result = execute();
      count_ = 0;
      executed_ = true;
      last_tag_ = tag;
      last_result_ = static_cast<uint8_t>(result);
    }
  } else if (op == 27) {
    count_ = 0;
    result = R::Ok;
  } else if (id >= 1 && id <= 253) {
    if (op == 20 && (p[5] == 1 || p[5] == 2) && p[4] <= 70 && p[4] + p[5] <= 71) {
      result = bus_.read(id, p[4], reply + 5, p[5]);
    } else if (op == 21 && !running && !active_ && !stop_pending_) {
      result = configure(id, p[4], be16(p + 6), p[5]);
      reply[5] = p[4];
      reply[6] = p[7];
    } else if (op == 22 && p[3] <= 254) {
      // STAGEではtag領域を加速度に使用し、id/opで確認する。
      uint8_t index = 0;
      while (index < count_ && staged_[index].id != id) ++index;
      if (index < 16) {
        staged_[index] = {id, p[3], be16(p + 4), 0, be16(p + 6)};
        if (index == count_) ++count_;
        result = R::Ok;
      }
    } else if (op == 24) {
      uint8_t data[15]{};
      result = bus_.read(id, 56, data, 15);
      if (result == R::Ok) {
        for (uint8_t part = 0; part < 4; ++part) {
          uint8_t frame[8] = {1, id, tag, part, 0, 0, 0, 0};
          for (uint8_t j = 0; j < 4 && part * 4 + j < 15; ++j) frame[4 + j] = data[part * 4 + j];
          emit_(0x327, frame);
        }
      }
    } else if (op == 25 && !running) {
      result = bus_.ping(id);
    }
  }
  reply[4] = static_cast<uint8_t>(result);
  reply[7] = bus_.lastServoError();
  if (result != R::Ok && running) stop();
  emit_(0x326, reply);
}
