#include "el05_motor.hpp"

namespace {
namespace codec = domain::el05;
}  // namespace

bool El05Motor::sendFrame(uint8_t comm_type, const uint8_t data[8]) {
  return bus_.sendExt(codec::buildCanId(comm_type, host_id_, motor_id_), data, 8);
}

bool El05Motor::enable() {
  const uint8_t data[8] = {};
  return sendFrame(codec::comm::ENABLE, data);
}

bool El05Motor::disable(bool clear_fault) {
  uint8_t data[8] = {};
  data[0] = clear_fault ? 1 : 0;
  return sendFrame(codec::comm::DISABLE, data);
}

bool El05Motor::setZero() {
  uint8_t data[8] = {};
  data[0] = 1;
  return sendFrame(codec::comm::SET_ZERO, data);
}

bool El05Motor::setRunMode(RunMode mode) {
  return writeParamU8(codec::param::RUN_MODE, static_cast<uint8_t>(mode));
}

bool El05Motor::setLocRef(float pos_rad) {
  return writeParamFloat(codec::param::LOC_REF, pos_rad);
}

bool El05Motor::setSpeedRef(float vel_rad_s) {
  return writeParamFloat(codec::param::SPD_REF, vel_rad_s);
}

bool El05Motor::setCurrentRef(float amp) {
  return writeParamFloat(codec::param::IQ_REF, amp);
}

bool El05Motor::writeParamFloat(uint16_t param, float value) {
  uint8_t data[8] = {};
  codec::encodeParamFloat(param, value, data);
  return sendFrame(codec::comm::WRITE_PARAM, data);
}

bool El05Motor::writeParamU8(uint16_t param, uint8_t value) {
  uint8_t data[8] = {};
  codec::encodeParamU8(param, value, data);
  return sendFrame(codec::comm::WRITE_PARAM, data);
}

bool El05Motor::requestParam(uint16_t param) {
  uint8_t data[8] = {};
  codec::encodeParamRequest(param, data);
  return sendFrame(codec::comm::READ_PARAM, data);
}

bool El05Motor::saveParams() {
  uint8_t data[8] = {};
  codec::encodeSave(data);
  return sendFrame(codec::comm::SAVE_PARAM, data);
}

bool El05Motor::onFeedback(uint32_t ext_id, const uint8_t data[8]) {
  switch (codec::commType(ext_id)) {
    case codec::comm::FEEDBACK: {
      // フィードバックは data_area_2 の下位 8bit に発信元のモータIDが載る。
      if (static_cast<uint8_t>(codec::dataArea2(ext_id) & 0xFF) != motor_id_) {
        return false;
      }
      feedback_ = codec::decodeFeedback(ext_id, data);
      return true;
    }
    case codec::comm::READ_PARAM: {
      // 応答の ID にはホスト側の CAN_ID が載る。マニュアルの記載が
      // フィードバックほど明確でないため、種別だけで受ける。
      has_param_reply_ = true;
      last_param_index_ = codec::paramReplyIndex(data);
      last_param_value_ = codec::paramReplyFloat(data);
      return true;
    }
    default:
      return false;
  }
}
