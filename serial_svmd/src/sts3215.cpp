#include "sts3215.hpp"

#include <cstring>

namespace {
namespace proto = domain::sts3215;

uint32_t remainingTimeout(uint32_t start_ms, uint32_t timeout_ms) {
  const uint32_t elapsed = HAL_GetTick() - start_ms;
  return elapsed >= timeout_ms ? 0 : timeout_ms - elapsed;
}
}  // namespace

void Sts3215::flushRx() {
  __HAL_UART_SEND_REQ(huart_, UART_RXDATA_FLUSH_REQUEST);
  __HAL_UART_CLEAR_OREFLAG(huart_);
}

Sts3215::Result Sts3215::receiveExact(uint8_t* data, uint16_t length, uint32_t start_ms) {
  const uint32_t remaining = remainingTimeout(start_ms, timeout_ms_);
  if (remaining == 0) {
    last_hal_status_ = HAL_TIMEOUT;
    return Result::Timeout;
  }

  last_hal_status_ = HAL_UART_Receive(huart_, data, length, remaining);
  if (last_hal_status_ == HAL_TIMEOUT) {
    return Result::Timeout;
  }
  if (last_hal_status_ != HAL_OK) {
    return Result::HalError;
  }
  return Result::Ok;
}

Sts3215::Result Sts3215::sendInstruction(uint8_t id, uint8_t instruction,
                                         const uint8_t* parameters,
                                         uint8_t parameter_count) {
  if (huart_ == nullptr) {
    return Result::ArgumentError;
  }

  uint8_t packet[proto::MAX_PACKET_SIZE];
  const std::size_t packet_length = proto::buildInstruction(
      packet, sizeof(packet), id, instruction, parameters, parameter_count);
  if (packet_length == 0) {
    return Result::ArgumentError;
  }

  // 半二重バスなので、送信前に自分の残響や前回の取りこぼしを捨てる。
  flushRx();
  last_hal_status_ = HAL_UART_Transmit(huart_, packet,
                                       static_cast<uint16_t>(packet_length), timeout_ms_);
  if (last_hal_status_ == HAL_TIMEOUT) {
    return Result::Timeout;
  }
  if (last_hal_status_ != HAL_OK) {
    return Result::HalError;
  }
  return Result::Ok;
}

Sts3215::Result Sts3215::receiveStatus(uint8_t expected_id, uint8_t* parameters,
                                       uint8_t capacity, uint8_t& parameter_count) {
  if (huart_ == nullptr) {
    return Result::ArgumentError;
  }

  const uint32_t start_ms = HAL_GetTick();

  // FF FF が2バイト続くまでを同期用に読み飛ばす。
  uint8_t header_count = 0;
  while (header_count < 2) {
    uint8_t byte = 0;
    const Result result = receiveExact(&byte, 1, start_ms);
    if (result != Result::Ok) {
      return result;
    }
    header_count = (byte == proto::HEADER) ? static_cast<uint8_t>(header_count + 1) : 0;
  }

  uint8_t prefix[2];
  Result result = receiveExact(prefix, sizeof(prefix), start_ms);
  if (result != Result::Ok) {
    return result;
  }

  const uint8_t response_id = prefix[0];
  const uint8_t response_length = prefix[1];
  if (response_id != expected_id || response_length < 2 ||
      response_length > (proto::MAX_RX_PARAMETERS + 2)) {
    return Result::ProtocolError;
  }

  // body = <エラービット> <パラメータ...> <チェックサム>
  uint8_t body[proto::MAX_RX_PARAMETERS + 2];
  result = receiveExact(body, response_length, start_ms);
  if (result != Result::Ok) {
    return result;
  }

  if (!proto::verifyStatusChecksum(response_id, response_length, body)) {
    return Result::ChecksumError;
  }

  last_servo_error_ = body[0];
  parameter_count = static_cast<uint8_t>(response_length - 2);
  if (parameter_count > capacity) {
    return Result::ProtocolError;
  }
  if (parameter_count != 0 && parameters != nullptr) {
    std::memcpy(parameters, &body[1], parameter_count);
  }

  return last_servo_error_ != 0 ? Result::ServoError : Result::Ok;
}

Sts3215::Result Sts3215::ping(uint8_t id) {
  const Result result = sendInstruction(id, proto::INSTRUCTION_PING, nullptr, 0);
  if (result != Result::Ok) {
    return result;
  }

  uint8_t parameter_count = 0;
  return receiveStatus(id, nullptr, 0, parameter_count);
}

Sts3215::Result Sts3215::read(uint8_t id, uint8_t address, uint8_t* data, uint8_t length) {
  if (data == nullptr || length == 0 || length > proto::MAX_RX_PARAMETERS) {
    return Result::ArgumentError;
  }

  const uint8_t parameters[2] = {address, length};
  Result result =
      sendInstruction(id, proto::INSTRUCTION_READ, parameters, sizeof(parameters));
  if (result != Result::Ok) {
    return result;
  }

  uint8_t received_count = 0;
  result = receiveStatus(id, data, length, received_count);
  if (result != Result::Ok) {
    return result;
  }

  return received_count == length ? Result::Ok : Result::ProtocolError;
}

Sts3215::Result Sts3215::write(uint8_t id, uint8_t address, const uint8_t* data,
                               uint8_t length) {
  if (data == nullptr || length == 0 || length >= proto::MAX_TX_PARAMETERS) {
    return Result::ArgumentError;
  }

  uint8_t parameters[proto::MAX_TX_PARAMETERS];
  parameters[0] = address;
  std::memcpy(&parameters[1], data, length);

  const Result result = sendInstruction(id, proto::INSTRUCTION_WRITE, parameters,
                                        static_cast<uint8_t>(length + 1));
  // ブロードキャスト宛にはステータスが返らない。
  if (result != Result::Ok || id == BROADCAST_ID || !wait_for_write_status_) {
    return result;
  }

  uint8_t received_count = 0;
  return receiveStatus(id, nullptr, 0, received_count);
}

Sts3215::Result Sts3215::setTorque(uint8_t id, bool enable) {
  const uint8_t value = enable ? 1 : 0;
  return write(id, proto::reg::TORQUE_ENABLE, &value, 1);
}

Sts3215::Result Sts3215::setTarget(const Target& target) {
  if (target.position > MAX_POSITION) {
    return Result::ArgumentError;
  }

  uint8_t data[proto::TARGET_DATA_LENGTH];
  proto::encodeTarget(target, data);
  return write(target.id, proto::reg::ACCELERATION, data, sizeof(data));
}

Sts3215::Result Sts3215::syncWriteTargets(const Target* targets, std::size_t count) {
  if (targets == nullptr || count == 0 || count > MAX_SYNC_TARGETS) {
    return Result::ArgumentError;
  }

  uint8_t parameters[proto::MAX_SYNC_PARAMETERS];
  parameters[0] = proto::reg::ACCELERATION;
  parameters[1] = proto::TARGET_DATA_LENGTH;

  std::size_t offset = 2;
  for (std::size_t i = 0; i < count; ++i) {
    if (targets[i].id == BROADCAST_ID || targets[i].position > MAX_POSITION) {
      return Result::ArgumentError;
    }

    parameters[offset++] = targets[i].id;
    proto::encodeTarget(targets[i], &parameters[offset]);
    offset += proto::TARGET_DATA_LENGTH;
  }

  return sendInstruction(BROADCAST_ID, proto::INSTRUCTION_SYNC_WRITE, parameters,
                         static_cast<uint8_t>(offset));
}

Sts3215::Result Sts3215::readPosition(uint8_t id, uint16_t& position) {
  uint8_t data[2];
  const Result result = read(id, proto::reg::PRESENT_POSITION, data, sizeof(data));
  if (result != Result::Ok) {
    return result;
  }

  position = proto::decodeUint16(data);
  return Result::Ok;
}
