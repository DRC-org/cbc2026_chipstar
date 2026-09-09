#include "sts3215.hpp"

#include <cstring>

namespace {
namespace proto = domain::sts3215;

uint32_t remainingTimeout(uint32_t start_ms, uint32_t timeout_ms) {
  const uint32_t elapsed = HAL_GetTick() - start_ms;
  return elapsed >= timeout_ms ? 0 : timeout_ms - elapsed;
}
}  // namespace

Sts3215::Result Sts3215::startReceiver() {
  if (!huart_ || !huart_->hdmarx) return Result::ArgumentError;
  rx_tail_ = 0;
  last_hal_status_ = HAL_UART_Receive_DMA(huart_, rx_buffer_, RX_BUFFER_SIZE);
  dma_rx_ = last_hal_status_ == HAL_OK;
  rx_restart_pending_ = !dma_rx_;
  if (dma_rx_) __HAL_DMA_DISABLE_IT(huart_->hdmarx, DMA_IT_HT | DMA_IT_TC);
  return dma_rx_ ? Result::Ok : Result::HalError;
}

void Sts3215::stopReceiver() {
  if (dma_rx_) HAL_UART_AbortReceive(huart_);
  dma_rx_ = false;
  rx_restart_pending_ = false;
}

void Sts3215::flushRx() {
  if (dma_rx_) {
    rx_tail_ = (RX_BUFFER_SIZE - __HAL_DMA_GET_COUNTER(huart_->hdmarx)) % RX_BUFFER_SIZE;
  } else {
    __HAL_UART_SEND_REQ(huart_, UART_RXDATA_FLUSH_REQUEST);
  }
  __HAL_UART_CLEAR_OREFLAG(huart_);
}

Sts3215::Result Sts3215::receiveExact(uint8_t* data, uint16_t length, uint32_t start_ms) {
  if (!dma_rx_) {
    const uint32_t remaining = remainingTimeout(start_ms, timeout_ms_);
    if (remaining == 0) { last_hal_status_ = HAL_TIMEOUT; return Result::Timeout; }
    last_hal_status_ = HAL_UART_Receive(huart_, data, length, remaining);
    return last_hal_status_ == HAL_OK ? Result::Ok :
           last_hal_status_ == HAL_TIMEOUT ? Result::Timeout : Result::HalError;
  }
  for (uint16_t i = 0; i < length;) {
    if (remainingTimeout(start_ms, timeout_ms_) == 0) {
      last_hal_status_ = HAL_TIMEOUT;
      rx_restart_pending_ = true;
      return Result::Timeout;
    }
    if (huart_->ErrorCode != HAL_UART_ERROR_NONE) {
      last_hal_status_ = HAL_ERROR;
      rx_restart_pending_ = true;
      return Result::HalError;
    }
    const uint16_t head = (RX_BUFFER_SIZE - __HAL_DMA_GET_COUNTER(huart_->hdmarx)) % RX_BUFFER_SIZE;
    if (head == rx_tail_) continue;
    data[i++] = reinterpret_cast<volatile uint8_t*>(rx_buffer_)[rx_tail_];
    rx_tail_ = (rx_tail_ + 1) % RX_BUFFER_SIZE;
  }
  last_hal_status_ = HAL_OK;
  return Result::Ok;
}

Sts3215::Result Sts3215::sendInstruction(uint8_t id, uint8_t instruction,
                                         const uint8_t* parameters,
                                         uint8_t parameter_count) {
  if (huart_ == nullptr || id == 0xFF) return Result::ArgumentError;
  last_servo_error_ = 0;
  if (rx_restart_pending_ || (dma_rx_ && huart_->ErrorCode != HAL_UART_ERROR_NONE)) {
    // DMA停止や応答途中のタイムアウト後は、再送前に受信を張り直す。
    // 再開失敗時も次の命令で再試行し、ポーリング受信へ暗黙に移行しない。
    stopReceiver();
    if (startReceiver() != Result::Ok) return Result::HalError;
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

  parameter_count = 0;
  // 他ID・古い書込みACK・壊れたフレームを捨て、同じ期限内で再同期する。
  Result last = Result::Timeout;
  while (remainingTimeout(start_ms, timeout_ms_) != 0) {
    uint8_t header_count = 0;
    uint8_t byte = 0;
    while (header_count < 2) {
      const auto result = receiveExact(&byte, 1, start_ms);
      if (result != Result::Ok) return result == Result::Timeout ? last : result;
      header_count = byte == proto::HEADER ? header_count + 1 : 0;
    }
    do {
      const auto result = receiveExact(&byte, 1, start_ms);
      if (result != Result::Ok) return result == Result::Timeout ? last : result;
    } while (byte == proto::HEADER);
    const uint8_t response_id = byte;
    uint8_t length = 0;
    auto result = receiveExact(&length, 1, start_ms);
    if (result != Result::Ok) return result;
    if (response_id > 253 || length < 2 || length > proto::MAX_RX_PARAMETERS + 2) {
      last = Result::ProtocolError; continue;
    }
    uint8_t body[proto::MAX_RX_PARAMETERS + 2];
    result = receiveExact(body, length, start_ms);
    if (result != Result::Ok) return result;
    if (!proto::verifyStatusChecksum(response_id, length, body)) {
      last = Result::ChecksumError; continue;
    }
    if (response_id != expected_id) continue;
    if (body[0] != 0) { last_servo_error_ = body[0]; return Result::ServoError; }
    if (length - 2 != capacity) { last = Result::ProtocolError; continue; }
    parameter_count = length - 2;
    if (parameter_count && parameters) std::memcpy(parameters, body + 1, parameter_count);
    return Result::Ok;
  }
  return last;
}

Sts3215::Result Sts3215::ping(uint8_t id) {
  if (id >= BROADCAST_ID) return Result::ArgumentError;
  const Result result = sendInstruction(id, proto::INSTRUCTION_PING, nullptr, 0);
  if (result != Result::Ok) {
    return result;
  }

  uint8_t parameter_count = 0;
  return receiveStatus(id, nullptr, 0, parameter_count);
}

Sts3215::Result Sts3215::read(uint8_t id, uint8_t address, uint8_t* data, uint8_t length) {
  Result result = Result::ArgumentError;
  for (uint8_t attempt = 0; attempt < 3; ++attempt) {
    if (attempt) HAL_Delay(5);
    result = readOnce(id, address, data, length);
    if (result != Result::Timeout && result != Result::HalError &&
        result != Result::ProtocolError && result != Result::ChecksumError) return result;
  }
  return result;
}

Sts3215::Result Sts3215::readOnce(uint8_t id, uint8_t address, uint8_t* data, uint8_t length) {
  if (id >= BROADCAST_ID || data == nullptr || length == 0 || length > proto::MAX_RX_PARAMETERS) {
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

  // 応答を待たない単体WRITEは遅延ACKと次のREADが衝突する。
  // SYNC_WRITEの1台指定なら仕様上ACKがなく、読戻しで確認できる。
  if (!wait_for_write_status_ && id < BROADCAST_ID) {
    if (length + 3 > proto::MAX_TX_PARAMETERS) return Result::ArgumentError;
    uint8_t sync[proto::MAX_TX_PARAMETERS] = {address, length, id};
    std::memcpy(sync + 3, data, length);
    return sendInstruction(BROADCAST_ID, proto::INSTRUCTION_SYNC_WRITE, sync, length + 3);
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
  return id == BROADCAST_ID ? write(id, proto::reg::TORQUE_ENABLE, &value, 1)
                            : writeAbsoluteVerified(id, proto::reg::TORQUE_ENABLE, &value, 1);
}

Sts3215::Result Sts3215::setTarget(const Target& target) {
  if (target.position > MAX_POSITION) {
    return Result::ArgumentError;
  }

  uint8_t data[proto::TARGET_DATA_LENGTH];
  proto::encodeTarget(target, data);
  return writeAbsoluteVerified(target.id, proto::reg::ACCELERATION, data, sizeof(data));
}

Sts3215::Result Sts3215::writeAbsoluteVerified(uint8_t id, uint8_t address,
                                             const uint8_t* data, uint8_t length) {
  // 絶対位置とトルクON/OFFは同じ値を再送しても作用が累積しない。
  // 相対ステップ、EEPROM、ID変更にはこの経路を使用しない。
  Result result = Result::ArgumentError;
  for (uint8_t attempt = 0; attempt < 3; ++attempt) {
    if (attempt) HAL_Delay(1);
    result = writeVerified(id, address, data, length);
    if (result != Result::Timeout && result != Result::HalError &&
        result != Result::ProtocolError && result != Result::ChecksumError &&
        result != Result::ReadbackMismatch) return result;
  }
  return result;
}

Sts3215::Result Sts3215::syncWriteTargets(const Target* targets, std::size_t count) {
  if (!targets || count == 0 || count > MAX_SYNC_TARGETS) return Result::ArgumentError;
  for (std::size_t i = 0; i < count; ++i) {
    if (targets[i].position > MAX_POSITION) return Result::ArgumentError;
  }
  return syncWriteRawTargets(targets, count);
}

Sts3215::Result Sts3215::syncWriteRawTargets(const Target* targets, std::size_t count) {
  if (targets == nullptr || count == 0 || count > MAX_SYNC_TARGETS) {
    return Result::ArgumentError;
  }

  uint8_t parameters[proto::MAX_SYNC_PARAMETERS];
  parameters[0] = proto::reg::ACCELERATION;
  parameters[1] = proto::TARGET_DATA_LENGTH;

  std::size_t offset = 2;
  for (std::size_t i = 0; i < count; ++i) {
    if (targets[i].id >= BROADCAST_ID) {
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
  return position <= MAX_POSITION ? Result::Ok : Result::ProtocolError;
}

Sts3215::Result Sts3215::writeVerified(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length) {
  if (id >= BROADCAST_ID || !data || !length || length > proto::MAX_RX_PARAMETERS) return Result::ArgumentError;
  auto result = write(id, address, data, length);
  if (result != Result::Ok) return result;
  return verifyReadback(id, address, data, length);
}

Sts3215::Result Sts3215::verifyReadback(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length) {
  if (id >= BROADCAST_ID || !data || !length || length > proto::MAX_RX_PARAMETERS) return Result::ArgumentError;
  uint8_t actual[proto::MAX_RX_PARAMETERS];
  // 無応答WRITE直後の反映遅延にはREADだけを再試行する。
  // 相対移動を二重実行しないよう、書込み自体は再送しない。
  for (uint8_t attempt = 0; attempt < 3; ++attempt) {
    if (attempt) HAL_Delay(1);
    const auto result = read(id, address, actual, length);
    if (result != Result::Ok) return result;
    if (std::memcmp(data, actual, length) == 0) return Result::Ok;
    for (uint8_t i = 0; i < length; ++i) {
      if (data[i] != actual[i]) {
        mismatch_ = {static_cast<uint8_t>(address + i), data[i], actual[i]};
        break;
      }
    }
  }
  return Result::ReadbackMismatch;
}

Sts3215::Result Sts3215::writeUnacknowledged(uint8_t id, uint8_t address, const uint8_t* data, uint8_t length) {
  if (id >= BROADCAST_ID || !data || !length || length + 3 > proto::MAX_TX_PARAMETERS) return Result::ArgumentError;
  uint8_t packet[proto::MAX_TX_PARAMETERS] = {address, length, id};
  std::memcpy(packet + 3, data, length);
  return sendInstruction(BROADCAST_ID, proto::INSTRUCTION_SYNC_WRITE, packet, length + 3);
}

Sts3215::Result Sts3215::preparePosition(uint8_t id, const Target* target) {
  auto result = setTorque(id, false);
  if (result != Result::Ok) return result;
  uint8_t mode = 0;
  result = read(id, 33, &mode, 1);
  if (result != Result::Ok) return result;
  if (mode != 0) return Result::UnsupportedMode;
  uint16_t position = 0;
  result = readPosition(id, position);
  if (result != Result::Ok) return result;
  const Target hold{id, 0, position, 0, 100};
  result = setTarget(target ? *target : hold);
  if (result != Result::Ok) return result;
  return setTorque(id, true);
}
