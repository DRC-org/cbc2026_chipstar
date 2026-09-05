#include "app.h"
#include "main.h"
#include "domain/controller.hpp"
#include "domain/encoder.hpp"

extern "C" {
extern CAN_HandleTypeDef hcan;
extern TIM_HandleTypeDef htim2;
extern TIM_HandleTypeDef htim3;
}

namespace {
dcmd::Controller controller;
bool bus_ready = false;
dcmd::Encoder encoder;
volatile uint16_t index_count = 0;
void sampleInputs() {
  const uint16_t a = GPIOA->IDR;
  const uint16_t b = GPIOB->IDR;
  const uint8_t dip = (!(a & GPIO_PIN_7) ? 1 : 0) | (!(a & GPIO_PIN_5) ? 2 : 0);
  controller.updateInputs(static_cast<uint8_t>((~b >> 3) & 7), dip, HAL_GetTick());
}
void output(TIM_HandleTypeDef* timer, int16_t duty) {
  // CCRプリロードを同じupdateイベントで反映し、方向切替を一括適用する。
  timer->Instance->CR1 |= TIM_CR1_UDIS;
  __HAL_TIM_SET_COMPARE(timer, TIM_CHANNEL_3, duty > 0 ? duty * 400 / 1000 : 0);
  __HAL_TIM_SET_COMPARE(timer, TIM_CHANNEL_4, duty < 0 ? -duty * 400 / 1000 : 0);
  timer->Instance->CR1 &= ~TIM_CR1_UDIS;
}
void send(uint16_t id, uint8_t* data) {
  CAN_TxHeaderTypeDef header = {};
  header.StdId = id;
  header.IDE = CAN_ID_STD;
  header.RTR = CAN_RTR_DATA;
  header.DLC = 8;
  uint32_t mailbox;
  if (HAL_CAN_GetTxMailboxesFreeLevel(&hcan)) HAL_CAN_AddTxMessage(&hcan, &header, data, &mailbox);
}
void status(uint8_t result) {
  uint8_t data[8] = {1, result, static_cast<uint8_t>(controller.mode()), controller.enabled(), 0, 0, 0, 0};
  for (uint8_t i = 0; i < 2; ++i) {
    const auto duty = static_cast<uint16_t>(controller.output(i));
    data[4 + i * 2] = static_cast<uint8_t>(duty >> 8);
    data[5 + i * 2] = static_cast<uint8_t>(duty);
  }
  send(dcmd::STATUS_ID, data);
}
void encoderStatus() {
  const uint32_t count = encoder.count();
  const uint16_t index = index_count;
  uint8_t data[8] = {1, 1, static_cast<uint8_t>(count >> 24),
    static_cast<uint8_t>(count >> 16), static_cast<uint8_t>(count >> 8),
    static_cast<uint8_t>(count), static_cast<uint8_t>(index >> 8), static_cast<uint8_t>(index)};
  send(dcmd::ENCODER_ID, data);
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
  output(&htim2, 0);
  if (HAL_TIM_PWM_Start(&htim2, TIM_CHANNEL_3) != HAL_OK ||
      HAL_TIM_PWM_Start(&htim2, TIM_CHANNEL_4) != HAL_OK) Error_Handler();
  HAL_Delay(100);  // ZIP検証済みのブートストラップ充電時間。
  __HAL_TIM_SET_COUNTER(&htim3, 0);
  if (HAL_TIM_Encoder_Start(&htim3, TIM_CHANNEL_ALL) != HAL_OK) Error_Handler();
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
  sampleInputs();
  encoder.sample(static_cast<uint16_t>(__HAL_TIM_GET_COUNTER(&htim3)));
  controller.tick(HAL_GetTick());
  for (uint8_t n = 0; n < 3 && HAL_CAN_GetRxFifoFillLevel(&hcan, CAN_RX_FIFO0); ++n) {
    CAN_RxHeaderTypeDef header = {};
    uint8_t data[8] = {};
    if (HAL_CAN_GetRxMessage(&hcan, CAN_RX_FIFO0, &header, data) != HAL_OK) break;
    dcmd::Command cmd;
    const bool accepted = header.IDE == CAN_ID_STD && header.RTR == CAN_RTR_DATA &&
        header.StdId == dcmd::COMMAND_ID && dcmd::parse(data, header.DLC, cmd) &&
        controller.apply(cmd, HAL_GetTick());
    if (accepted && cmd.op == dcmd::Op::InputRead) {
      const domain::DigitalInputs& in = controller.inputs();
      uint8_t report[8] = {1, in.raw(), in.stable(), in.dip(), in.available(), 0, 0, 0};
      send(dcmd::INPUT_ID, report);
    } else status(accepted ? 0 : 1);
  }
  output(&htim2, controller.output(0));
  static uint32_t last_status = 0;
  if (HAL_GetTick() - last_status >= 50) {
    last_status = HAL_GetTick();
    // USB CDCの連続通知欠落を避けるため、DutyとENCを交互に送る。
    static bool send_encoder = false;
    send_encoder = !send_encoder;
    if (send_encoder) encoderStatus();
    else status(controller.timedOut() ? 2 : 0);
  }
}

extern "C" void HAL_GPIO_EXTI_Callback(uint16_t pin) {
  if (pin == GPIO_PIN_7) ++index_count;
}
