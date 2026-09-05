#include "app.h"
#include "main.h"
#include "domain/controller.hpp"
#include <initializer_list>

extern "C" {
extern CAN_HandleTypeDef hcan;
extern TIM_HandleTypeDef htim2;
extern TIM_HandleTypeDef htim3;
}

namespace {
dcmd::Controller controller;
bool bus_ready = false;
void output(TIM_HandleTypeDef* timer, int16_t duty) {
  // CCRプリロードを同じupdateイベントで反映し、方向切替を一括適用する。
  timer->Instance->CR1 |= TIM_CR1_UDIS;
  __HAL_TIM_SET_COMPARE(timer, TIM_CHANNEL_3, duty > 0 ? duty * 400 / 1000 : 0);
  __HAL_TIM_SET_COMPARE(timer, TIM_CHANNEL_4, duty < 0 ? -duty * 400 / 1000 : 0);
  timer->Instance->CR1 &= ~TIM_CR1_UDIS;
}
void status(uint8_t result) {
  uint8_t data[8] = {1, result, static_cast<uint8_t>(controller.mode()), controller.enabled(), 0, 0, 0, 0};
  for (uint8_t i = 0; i < 2; ++i) {
    const auto duty = static_cast<uint16_t>(controller.output(i));
    data[4 + i * 2] = static_cast<uint8_t>(duty >> 8);
    data[5 + i * 2] = static_cast<uint8_t>(duty);
  }
  CAN_TxHeaderTypeDef header = {};
  header.StdId = dcmd::STATUS_ID;
  header.IDE = CAN_ID_STD;
  header.RTR = CAN_RTR_DATA;
  header.DLC = 8;
  uint32_t mailbox;
  if (HAL_CAN_GetTxMailboxesFreeLevel(&hcan)) HAL_CAN_AddTxMessage(&hcan, &header, data, &mailbox);
}
}

extern "C" void dcmd_brake(void) {
  // タイマ初期化失敗時にもGPIOでアクティブLow入力を非駆動にする。
  __HAL_RCC_GPIOA_CLK_ENABLE();
  __HAL_RCC_GPIOB_CLK_ENABLE();
  HAL_GPIO_WritePin(GPIOA, GPIO_PIN_2 | GPIO_PIN_3, GPIO_PIN_SET);
  HAL_GPIO_WritePin(GPIOB, GPIO_PIN_0 | GPIO_PIN_1, GPIO_PIN_SET);
  GPIO_InitTypeDef gpio = {};
  gpio.Mode = GPIO_MODE_OUTPUT_PP;
  gpio.Pin = GPIO_PIN_2 | GPIO_PIN_3;
  HAL_GPIO_Init(GPIOA, &gpio);
  gpio.Pin = GPIO_PIN_0 | GPIO_PIN_1;
  HAL_GPIO_Init(GPIOB, &gpio);
}

extern "C" void setup(void) {
  for (auto* timer : {&htim2, &htim3}) {
    output(timer, 0);
    if (HAL_TIM_PWM_Start(timer, TIM_CHANNEL_3) != HAL_OK ||
        HAL_TIM_PWM_Start(timer, TIM_CHANNEL_4) != HAL_OK) Error_Handler();
  }
  HAL_Delay(100);  // ZIP検証済みのブートストラップ充電時間。
  CAN_FilterTypeDef filter = {};
  filter.FilterMode = CAN_FILTERMODE_IDMASK;
  filter.FilterScale = CAN_FILTERSCALE_32BIT;
  filter.FilterIdHigh = dcmd::COMMAND_ID << 5;
  filter.FilterMaskIdHigh = 0x7FF << 5;
  filter.FilterMaskIdLow = 6;  // 標準ID、データフレームのみ。
  filter.FilterActivation = ENABLE;
  bus_ready = HAL_CAN_ConfigFilter(&hcan, &filter) == HAL_OK && HAL_CAN_Start(&hcan) == HAL_OK;
  if (!bus_ready) Error_Handler();
}

extern "C" void loop(void) {
  controller.tick(HAL_GetTick());
  for (uint8_t n = 0; n < 3 && HAL_CAN_GetRxFifoFillLevel(&hcan, CAN_RX_FIFO0); ++n) {
    CAN_RxHeaderTypeDef header = {};
    uint8_t data[8] = {};
    if (HAL_CAN_GetRxMessage(&hcan, CAN_RX_FIFO0, &header, data) != HAL_OK) break;
    dcmd::Command cmd;
    const bool accepted = header.IDE == CAN_ID_STD && header.RTR == CAN_RTR_DATA &&
        header.StdId == dcmd::COMMAND_ID && dcmd::parse(data, header.DLC, cmd) &&
        controller.apply(cmd, HAL_GetTick());
    status(accepted ? 0 : 1);
  }
  output(&htim2, controller.output(0));
  output(&htim3, controller.output(1));
  static uint32_t last_status = 0;
  if (HAL_GetTick() - last_status >= 50) {
    last_status = HAL_GetTick();
    status(controller.timedOut() ? 2 : 0);
  }
}
