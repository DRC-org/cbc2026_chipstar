#include "sts3215.hpp"

#include <cstring>

namespace {
constexpr uint8_t HEADER = 0xFF;
constexpr uint8_t INSTRUCTION_PING = 0x01;
constexpr uint8_t INSTRUCTION_READ = 0x02;
constexpr uint8_t INSTRUCTION_WRITE = 0x03;
constexpr uint8_t INSTRUCTION_SYNC_WRITE = 0x83;

constexpr uint8_t MAX_TX_PARAMETERS = 132;
constexpr uint8_t MAX_RX_PARAMETERS = 64;
// 加速度・目標位置・目標時間・目標速度の合計バイト数。
constexpr uint8_t TARGET_DATA_LENGTH = 7;

// ID からチェックサム直前までの総和のビット反転。
uint8_t checksum(const uint8_t* data, std::size_t length) {
  uint16_t sum = 0;
  for (std::size_t i = 0; i < length; ++i) {
    sum = static_cast<uint16_t>(sum + data[i]);
  }
  return static_cast<uint8_t>(~sum);
}

uint32_t remainingTimeout(uint32_t start_ms, uint32_t timeout_ms) {
  const uint32_t elapsed = HAL_GetTick() - start_ms;
  return elapsed >= timeout_ms ? 0 : timeout_ms - elapsed;
}

void encodeTargetData(const Sts3215::Target& target, uint8_t* data) {
  // STS 仕様どおり16ビット値はリトルエンディアン。
  data[0] = target.acceleration;
  data[1] = static_cast<uint8_t>(target.position & 0xFF);
  data[2] = static_cast<uint8_t>(target.position >> 8);
  data[3] = static_cast<uint8_t>(target.time & 0xFF);
  data[4] = static_cast<uint8_t>(target.time >> 8);
  data[5] = static_cast<uint8_t>(target.speed & 0xFF);
  data[6] = static_cast<uint8_t>(target.speed >> 8);
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
  if (huart_ == nullptr || (parameters == nullptr && parameter_count != 0) ||
      parameter_count > MAX_TX_PARAMETERS) {
    return Result::ArgumentError;
  }

  // FF FF <ID> <長さ> <命令> <パラメータ...> <チェックサム>
  uint8_t packet[MAX_TX_PARAMETERS + 6];
  const uint16_t packet_length = static_cast<uint16_t>(parameter_count) + 6;

  packet[0] = HEADER;
  packet[1] = HEADER;
  packet[2] = id;
  packet[3] = static_cast<uint8_t>(parameter_count + 2);
  packet[4] = instruction;
  if (parameter_count != 0) {
    std::memcpy(&packet[5], parameters, parameter_count);
  }
  packet[packet_length - 1] = checksum(&packet[2], packet_length - 3);

  // 半二重バスなので、送信前に自分の残響や前回の取りこぼしを捨てる。
  flushRx();
  last_hal_status_ = HAL_UART_Transmit(huart_, packet, packet_length, timeout_ms_);
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
    header_count = (byte == HEADER) ? static_cast<uint8_t>(header_count + 1) : 0;
  }

  uint8_t prefix[2];
  Result result = receiveExact(prefix, sizeof(prefix), start_ms);
  if (result != Result::Ok) {
    return result;
  }

  const uint8_t response_id = prefix[0];
  const uint8_t response_length = prefix[1];
  if (response_id != expected_id || response_length < 2 ||
      response_length > (MAX_RX_PARAMETERS + 2)) {
    return Result::ProtocolError;
  }

  // body = <エラービット> <パラメータ...> <チェックサム>
  uint8_t body[MAX_RX_PARAMETERS + 2];
  result = receiveExact(body, response_length, start_ms);
  if (result != Result::Ok) {
    return result;
  }

  uint8_t checksum_data[MAX_RX_PARAMETERS + 3];
  checksum_data[0] = response_id;
  checksum_data[1] = response_length;
  std::memcpy(&checksum_data[2], body, response_length - 1);
  if (checksum(checksum_data, static_cast<std::size_t>(response_length) + 1) !=
      body[response_length - 1]) {
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
  const Result result = sendInstruction(id, INSTRUCTION_PING, nullptr, 0);
  if (result != Result::Ok) {
    return result;
  }

  uint8_t parameter_count = 0;
  return receiveStatus(id, nullptr, 0, parameter_count);
}

Sts3215::Result Sts3215::read(uint8_t id, uint8_t address, uint8_t* data, uint8_t length) {
  if (data == nullptr || length == 0 || length > MAX_RX_PARAMETERS) {
    return Result::ArgumentError;
  }

  const uint8_t parameters[2] = {address, length};
  Result result = sendInstruction(id, INSTRUCTION_READ, parameters, sizeof(parameters));
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
  if (data == nullptr || length == 0 || length >= MAX_TX_PARAMETERS) {
    return Result::ArgumentError;
  }

  uint8_t parameters[MAX_TX_PARAMETERS];
  parameters[0] = address;
  std::memcpy(&parameters[1], data, length);

  const Result result =
      sendInstruction(id, INSTRUCTION_WRITE, parameters, static_cast<uint8_t>(length + 1));
  // ブロードキャスト宛にはステータスが返らない。
  if (result != Result::Ok || id == BROADCAST_ID || !wait_for_write_status_) {
    return result;
  }

  uint8_t received_count = 0;
  return receiveStatus(id, nullptr, 0, received_count);
}

Sts3215::Result Sts3215::setTorque(uint8_t id, bool enable) {
  const uint8_t value = enable ? 1 : 0;
  return write(id, sts3215_reg::TORQUE_ENABLE, &value, 1);
}

Sts3215::Result Sts3215::setTarget(const Target& target) {
  if (target.position > MAX_POSITION) {
    return Result::ArgumentError;
  }

  uint8_t data[TARGET_DATA_LENGTH];
  encodeTargetData(target, data);
  return write(target.id, sts3215_reg::ACCELERATION, data, sizeof(data));
}

Sts3215::Result Sts3215::syncWriteTargets(const Target* targets, std::size_t count) {
  if (targets == nullptr || count == 0 || count > MAX_SYNC_TARGETS) {
    return Result::ArgumentError;
  }

  uint8_t parameters[2 + (MAX_SYNC_TARGETS * (TARGET_DATA_LENGTH + 1))];
  parameters[0] = sts3215_reg::ACCELERATION;
  parameters[1] = TARGET_DATA_LENGTH;

  std::size_t offset = 2;
  for (std::size_t i = 0; i < count; ++i) {
    if (targets[i].id == BROADCAST_ID || targets[i].position > MAX_POSITION) {
      return Result::ArgumentError;
    }

    parameters[offset++] = targets[i].id;
    encodeTargetData(targets[i], &parameters[offset]);
    offset += TARGET_DATA_LENGTH;
  }

  return sendInstruction(BROADCAST_ID, INSTRUCTION_SYNC_WRITE, parameters,
                         static_cast<uint8_t>(offset));
}

Sts3215::Result Sts3215::readPosition(uint8_t id, uint16_t& position) {
  uint8_t data[2];
  const Result result = read(id, sts3215_reg::PRESENT_POSITION, data, sizeof(data));
  if (result != Result::Ok) {
    return result;
  }

  position = static_cast<uint16_t>(data[0]) | static_cast<uint16_t>(data[1] << 8);
  return Result::Ok;
}
