#pragma once
#include <cstdint>
enum HAL_StatusTypeDef { HAL_OK=0, HAL_ERROR=1, HAL_BUSY=2, HAL_TIMEOUT=3 };
enum GPIO_PinState { GPIO_PIN_RESET=0, GPIO_PIN_SET=1 };
struct DMA_HandleTypeDef { uint16_t count=256; };
struct UART_HandleTypeDef { DMA_HandleTypeDef* hdmarx; uint32_t ErrorCode=0; };
struct GPIO_TypeDef {};
extern GPIO_TypeDef fake_gpio;
#define USART_SSV_RX_GPIO_Port (&fake_gpio)
#define USART_SSV_RX_Pin 10
constexpr uint32_t HAL_UART_ERROR_NONE=0, UART_RXDATA_FLUSH_REQUEST=0, DMA_IT_HT=1, DMA_IT_TC=2;
#define __HAL_DMA_GET_COUNTER(h) ((h)->count)
#define __HAL_DMA_DISABLE_IT(h,m) ((void)0)
#define __HAL_UART_SEND_REQ(h,m) ((void)0)
#define __HAL_UART_CLEAR_OREFLAG(h) ((void)0)
uint32_t HAL_GetTick();
void HAL_Delay(uint32_t);
HAL_StatusTypeDef HAL_UART_Transmit(UART_HandleTypeDef*,uint8_t*,uint16_t,uint32_t);
HAL_StatusTypeDef HAL_UART_Receive(UART_HandleTypeDef*,uint8_t*,uint16_t,uint32_t);
HAL_StatusTypeDef HAL_UART_Receive_DMA(UART_HandleTypeDef*,uint8_t*,uint16_t);
HAL_StatusTypeDef HAL_UART_AbortReceive(UART_HandleTypeDef*);
GPIO_PinState HAL_GPIO_ReadPin(GPIO_TypeDef*, uint16_t);
