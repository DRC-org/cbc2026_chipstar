#include "main.h"

#include <cstdint>

extern "C" {
extern FDCAN_HandleTypeDef hfdcan1;
}

namespace {
void haltOnError() {
  while (true) {
    HAL_Delay(100);
  }
}

void startFdcan1() {
  if (HAL_FDCAN_ConfigGlobalFilter(&hfdcan1, FDCAN_ACCEPT_IN_RX_FIFO0,
                                   FDCAN_ACCEPT_IN_RX_FIFO0, FDCAN_REJECT_REMOTE,
                                   FDCAN_REJECT_REMOTE) != HAL_OK) {
    haltOnError();
  }

  if (HAL_FDCAN_Start(&hfdcan1) != HAL_OK) {
    haltOnError();
  }
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
  (void)data;
  (void)length;
}
}

extern "C" void setup(void) {
  startFdcan1();
}

extern "C" void loop(void) {
  if (HAL_FDCAN_GetRxFifoFillLevel(&hfdcan1, FDCAN_RX_FIFO0) == 0U) {
    return;
  }

  FDCAN_RxHeaderTypeDef rx_header = {};
  uint8_t rx_data[8] = {};
  if (HAL_FDCAN_GetRxMessage(&hfdcan1, FDCAN_RX_FIFO0, &rx_header, rx_data) != HAL_OK) {
    haltOnError();
  }

  FDCAN_TxHeaderTypeDef tx_header = {};
  tx_header.Identifier = rx_header.Identifier;
  tx_header.IdType = rx_header.IdType;
  tx_header.TxFrameType = rx_header.RxFrameType;
  tx_header.DataLength = rx_header.DataLength;
  tx_header.ErrorStateIndicator = FDCAN_ESI_ACTIVE;
  tx_header.BitRateSwitch = FDCAN_BRS_OFF;
  tx_header.FDFormat = FDCAN_CLASSIC_CAN;
  tx_header.TxEventFifoControl = FDCAN_NO_TX_EVENTS;

  if (HAL_FDCAN_AddMessageToTxFifoQ(&hfdcan1, &tx_header, rx_data) != HAL_OK) {
    haltOnError();
  }
}
