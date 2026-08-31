// CAN 受信の疎通確認サンプル。
//
// 標準データフレーム ID 0x123 / DLC 5 / ASCII "hello" だけをフィルタで通し、
// 受信状況をデバッガの Live Expressions 用変数に反映する。
// ビットレートは .ioc の設定（1 Mbit/s）に従うので、送信側も合わせること。
//
// g_can_status_code:
//   0 起動処理中 / 1 CAN 開始済み・受信待ち / 2 正常受信 /
//   3 ID 通過後に DLC またはデータが不一致 / 4 HAL の FIFO 受信失敗 /
//   5 Error Warning / 6 Error Passive / 7 Bus-Off / 8 CAN 初期化・開始失敗

#include "main.h"

#include <cstdint>
#include <cstring>

extern "C" {
extern CAN_HandleTypeDef hcan;
}

namespace {
constexpr uint32_t EXPECTED_ID = 0x123;
constexpr uint8_t EXPECTED_DLC = 5;
constexpr char EXPECTED_DATA[] = "hello";

constexpr uint32_t STATUS_BOOT = 0;
constexpr uint32_t STATUS_WAITING = 1;
constexpr uint32_t STATUS_RX_OK = 2;
constexpr uint32_t STATUS_FRAME_MISMATCH = 3;
constexpr uint32_t STATUS_HAL_RX_ERROR = 4;
constexpr uint32_t STATUS_ERROR_WARNING = 5;
constexpr uint32_t STATUS_ERROR_PASSIVE = 6;
constexpr uint32_t STATUS_BUS_OFF = 7;
constexpr uint32_t STATUS_INIT_ERROR = 8;

uint32_t previous_bus_state = 0;

void haltOnError() {
  __disable_irq();
  while (true) {
  }
}
}  // namespace

// Live Expressions に登録して確認する変数。
volatile uint32_t g_can_status_code = STATUS_BOOT;
volatile uint32_t g_can_started = 0;
volatile uint32_t g_can_rx_total = 0;
volatile uint32_t g_can_success_count = 0;
volatile uint32_t g_can_failure_count = 0;
volatile uint32_t g_can_rx_invalid = 0;
volatile uint32_t g_can_error_events = 0;
volatile uint32_t g_can_last_frame_ok = 0;
volatile uint32_t g_can_last_tick_ms = 0;
volatile uint32_t g_can_last_message_age_ms = 0xFFFFFFFF;
volatile uint32_t g_can_last_id = 0;
volatile uint32_t g_can_error_code = 0;
volatile uint32_t g_can_esr = 0;
volatile uint32_t g_can_rec = 0;
volatile uint32_t g_can_tec = 0;
volatile uint32_t g_can_lec = 0;
volatile uint32_t g_can_fifo0_level = 0;
volatile uint8_t g_can_last_dlc = 0;
volatile uint8_t g_can_last_data[8] = {};

extern "C" void setup(void) {
  // 32bit マスクフィルタ。ID に加えて IDE/RTR ビットも比較し、
  // 標準データフレームの 0x123 だけを FIFO0 へ入れる。
  CAN_FilterTypeDef filter = {};
  filter.FilterBank = 0;
  filter.FilterMode = CAN_FILTERMODE_IDMASK;
  filter.FilterScale = CAN_FILTERSCALE_32BIT;
  filter.FilterIdHigh = EXPECTED_ID << 5;
  filter.FilterIdLow = 0x0000;
  filter.FilterMaskIdHigh = 0xFFE0;
  filter.FilterMaskIdLow = 0x0006;
  filter.FilterFIFOAssignment = CAN_RX_FIFO0;
  filter.FilterActivation = ENABLE;
  filter.SlaveStartFilterBank = 14;

  if (HAL_CAN_ConfigFilter(&hcan, &filter) != HAL_OK ||
      HAL_CAN_Start(&hcan) != HAL_OK) {
    g_can_status_code = STATUS_INIT_ERROR;
    ++g_can_failure_count;
    ++g_can_error_events;
    haltOnError();
  }

  g_can_started = 1;
  g_can_status_code = STATUS_WAITING;
}

extern "C" void loop(void) {
  while (HAL_CAN_GetRxFifoFillLevel(&hcan, CAN_RX_FIFO0) > 0) {
    CAN_RxHeaderTypeDef header = {};
    uint8_t data[8] = {};

    if (HAL_CAN_GetRxMessage(&hcan, CAN_RX_FIFO0, &header, data) != HAL_OK) {
      ++g_can_rx_invalid;
      ++g_can_failure_count;
      ++g_can_error_events;
      g_can_error_code = HAL_CAN_GetError(&hcan);
      g_can_last_frame_ok = 0;
      g_can_status_code = STATUS_HAL_RX_ERROR;
      break;
    }

    ++g_can_rx_total;
    g_can_last_tick_ms = HAL_GetTick();
    g_can_last_id = (header.IDE == CAN_ID_STD) ? header.StdId : header.ExtId;
    g_can_last_dlc = static_cast<uint8_t>(header.DLC);
    for (uint32_t i = 0; i < 8; ++i) {
      g_can_last_data[i] = data[i];
    }

    if (header.IDE == CAN_ID_STD && header.StdId == EXPECTED_ID &&
        header.RTR == CAN_RTR_DATA && header.DLC == EXPECTED_DLC &&
        std::memcmp(data, EXPECTED_DATA, EXPECTED_DLC) == 0) {
      ++g_can_success_count;
      g_can_last_frame_ok = 1;
    } else {
      ++g_can_rx_invalid;
      ++g_can_failure_count;
      g_can_last_frame_ok = 0;
    }
  }

  const uint32_t now = HAL_GetTick();
  g_can_error_code = HAL_CAN_GetError(&hcan);
  g_can_esr = READ_REG(hcan.Instance->ESR);
  g_can_rec = (g_can_esr & CAN_ESR_REC_Msk) >> CAN_ESR_REC_Pos;
  g_can_tec = (g_can_esr & CAN_ESR_TEC_Msk) >> CAN_ESR_TEC_Pos;
  g_can_lec = (g_can_esr & CAN_ESR_LEC_Msk) >> CAN_ESR_LEC_Pos;
  g_can_fifo0_level = HAL_CAN_GetRxFifoFillLevel(&hcan, CAN_RX_FIFO0);

  if (g_can_rx_total > 0) {
    g_can_last_message_age_ms = now - g_can_last_tick_ms;
  }

  const uint32_t bus_state = g_can_esr & (CAN_ESR_EWGF | CAN_ESR_EPVF | CAN_ESR_BOFF);
  if (bus_state != 0 && bus_state != previous_bus_state) {
    ++g_can_failure_count;
    ++g_can_error_events;
  }
  previous_bus_state = bus_state;

  if ((bus_state & CAN_ESR_BOFF) != 0) {
    g_can_status_code = STATUS_BUS_OFF;
  } else if ((bus_state & CAN_ESR_EPVF) != 0) {
    g_can_status_code = STATUS_ERROR_PASSIVE;
  } else if ((bus_state & CAN_ESR_EWGF) != 0) {
    g_can_status_code = STATUS_ERROR_WARNING;
  } else if (g_can_rx_total == 0) {
    g_can_status_code = STATUS_WAITING;
  } else if (g_can_last_frame_ok != 0) {
    g_can_status_code = STATUS_RX_OK;
  } else {
    g_can_status_code = STATUS_FRAME_MISMATCH;
  }
}
