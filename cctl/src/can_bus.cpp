#include "can_bus.hpp"

namespace {
uint32_t dlcFromLen(uint8_t len) {
  switch (len) {
    case 0: return FDCAN_DLC_BYTES_0;
    case 1: return FDCAN_DLC_BYTES_1;
    case 2: return FDCAN_DLC_BYTES_2;
    case 3: return FDCAN_DLC_BYTES_3;
    case 4: return FDCAN_DLC_BYTES_4;
    case 5: return FDCAN_DLC_BYTES_5;
    case 6: return FDCAN_DLC_BYTES_6;
    case 7: return FDCAN_DLC_BYTES_7;
    default: return FDCAN_DLC_BYTES_8;
  }
}
}  // namespace

bool CanBus::begin() {
  if (HAL_FDCAN_ConfigGlobalFilter(hcan_, FDCAN_ACCEPT_IN_RX_FIFO0,
                                   FDCAN_ACCEPT_IN_RX_FIFO0, FDCAN_REJECT_REMOTE,
                                   FDCAN_REJECT_REMOTE) != HAL_OK) {
    return false;
  }
  return HAL_FDCAN_Start(hcan_) == HAL_OK;
}

bool CanBus::send(uint32_t id, uint32_t id_type, const uint8_t* data, uint8_t len) {
  FDCAN_TxHeaderTypeDef tx_header = {};
  tx_header.Identifier = id;
  tx_header.IdType = id_type;
  tx_header.TxFrameType = FDCAN_DATA_FRAME;
  tx_header.DataLength = dlcFromLen(len);
  tx_header.ErrorStateIndicator = FDCAN_ESI_ACTIVE;
  tx_header.BitRateSwitch = FDCAN_BRS_OFF;
  tx_header.FDFormat = FDCAN_CLASSIC_CAN;
  tx_header.TxEventFifoControl = FDCAN_NO_TX_EVENTS;
  tx_header.MessageMarker = 0;

  return HAL_FDCAN_AddMessageToTxFifoQ(hcan_, &tx_header, const_cast<uint8_t*>(data)) == HAL_OK;
}

bool CanBus::sendStd(uint16_t id, const uint8_t* data, uint8_t len) {
  return send(id, FDCAN_STANDARD_ID, data, len);
}

bool CanBus::sendExt(uint32_t id, const uint8_t* data, uint8_t len) {
  return send(id, FDCAN_EXTENDED_ID, data, len);
}

bool CanBus::receive(domain::CanFrame& frame) {
  if (HAL_FDCAN_GetRxFifoFillLevel(hcan_, FDCAN_RX_FIFO0) == 0U) {
    return false;
  }

  FDCAN_RxHeaderTypeDef header = {};
  if (HAL_FDCAN_GetRxMessage(hcan_, FDCAN_RX_FIFO0, &header, frame.data) != HAL_OK) {
    return false;
  }

  frame.id = header.Identifier;
  frame.extended = header.IdType == FDCAN_EXTENDED_ID;
  // Classic CAN の DataLength はバイト数そのもの。
  frame.length = static_cast<uint8_t>(header.DataLength);
  return true;
}
