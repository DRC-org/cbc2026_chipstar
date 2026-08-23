/* USER CODE BEGIN Header */
/**
  ******************************************************************************
  * @file           : main.h
  * @brief          : Header for main.c file.
  *                   This file contains the common defines of the application.
  ******************************************************************************
  * @attention
  *
  * Copyright (c) 2026 STMicroelectronics.
  * All rights reserved.
  *
  * This software is licensed under terms that can be found in the LICENSE file
  * in the root directory of this software component.
  * If no LICENSE file comes with this software, it is provided AS-IS.
  *
  ******************************************************************************
  */
/* USER CODE END Header */

/* Define to prevent recursive inclusion -------------------------------------*/
#ifndef __MAIN_H
#define __MAIN_H

#ifdef __cplusplus
extern "C" {
#endif

/* Includes ------------------------------------------------------------------*/
#include "stm32f3xx_hal.h"

/* Private includes ----------------------------------------------------------*/
/* USER CODE BEGIN Includes */

/* USER CODE END Includes */

/* Exported types ------------------------------------------------------------*/
/* USER CODE BEGIN ET */

/* USER CODE END ET */

/* Exported constants --------------------------------------------------------*/
/* USER CODE BEGIN EC */

/* USER CODE END EC */

/* Exported macro ------------------------------------------------------------*/
/* USER CODE BEGIN EM */

/* USER CODE END EM */

/* Exported functions prototypes ---------------------------------------------*/
void Error_Handler(void);

/* USER CODE BEGIN EFP */

/* USER CODE END EFP */

/* Private defines -----------------------------------------------------------*/
#define SW6_Pin GPIO_PIN_4
#define SW6_GPIO_Port GPIOA
#define SW5_Pin GPIO_PIN_5
#define SW5_GPIO_Port GPIOA
#define SW4_Pin GPIO_PIN_6
#define SW4_GPIO_Port GPIOA
#define SW3_Pin GPIO_PIN_7
#define SW3_GPIO_Port GPIOA
#define SW2_Pin GPIO_PIN_0
#define SW2_GPIO_Port GPIOB
#define SW1_Pin GPIO_PIN_1
#define SW1_GPIO_Port GPIOB
#define USART_SSV_TX_Pin GPIO_PIN_9
#define USART_SSV_TX_GPIO_Port GPIOA
#define USART_SSV_RX_Pin GPIO_PIN_10
#define USART_SSV_RX_GPIO_Port GPIOA
#define USART_USB_RX_Pin GPIO_PIN_15
#define USART_USB_RX_GPIO_Port GPIOA
#define USART_USB_TX_Pin GPIO_PIN_3
#define USART_USB_TX_GPIO_Port GPIOB
#define DIP1_Pin GPIO_PIN_4
#define DIP1_GPIO_Port GPIOB
#define DIP2_Pin GPIO_PIN_5
#define DIP2_GPIO_Port GPIOB
#define DIP3_Pin GPIO_PIN_6
#define DIP3_GPIO_Port GPIOB
#define DIP4_Pin GPIO_PIN_7
#define DIP4_GPIO_Port GPIOB

/* USER CODE BEGIN Private defines */

/* USER CODE END Private defines */

#ifdef __cplusplus
}
#endif

#endif /* __MAIN_H */
