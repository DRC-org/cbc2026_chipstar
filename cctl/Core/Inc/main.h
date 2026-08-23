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
#include "stm32g4xx_hal.h"

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

void HAL_TIM_MspPostInit(TIM_HandleTypeDef *htim);

/* Exported functions prototypes ---------------------------------------------*/
void Error_Handler(void);

/* USER CODE BEGIN EFP */

/* USER CODE END EFP */

/* Private defines -----------------------------------------------------------*/
#define DIP1_Pin GPIO_PIN_14
#define DIP1_GPIO_Port GPIOC
#define DIP2_Pin GPIO_PIN_15
#define DIP2_GPIO_Port GPIOC
#define DIP3_Pin GPIO_PIN_0
#define DIP3_GPIO_Port GPIOF
#define DIP4_Pin GPIO_PIN_1
#define DIP4_GPIO_Port GPIOF
#define NRST_Pin GPIO_PIN_10
#define NRST_GPIO_Port GPIOG
#define LED1_Pin GPIO_PIN_1
#define LED1_GPIO_Port GPIOC
#define LED2_Pin GPIO_PIN_2
#define LED2_GPIO_Port GPIOC
#define LED3_Pin GPIO_PIN_3
#define LED3_GPIO_Port GPIOC
#define I2C1_INT_Pin GPIO_PIN_0
#define I2C1_INT_GPIO_Port GPIOB
#define I2C2_INT_Pin GPIO_PIN_1
#define I2C2_INT_GPIO_Port GPIOB
#define BZ_TIM15_CH2_Pin GPIO_PIN_15
#define BZ_TIM15_CH2_GPIO_Port GPIOB
#define I2C2_SCL_Pin GPIO_PIN_6
#define I2C2_SCL_GPIO_Port GPIOC
#define ISC2_SDA_Pin GPIO_PIN_7
#define ISC2_SDA_GPIO_Port GPIOC
#define I2C1_SCL_Pin GPIO_PIN_8
#define I2C1_SCL_GPIO_Port GPIOC
#define I2C1_SDA_Pin GPIO_PIN_9
#define I2C1_SDA_GPIO_Port GPIOC
#define SW3_Pin GPIO_PIN_8
#define SW3_GPIO_Port GPIOA
#define SW2_Pin GPIO_PIN_9
#define SW2_GPIO_Port GPIOA
#define SW1_Pin GPIO_PIN_10
#define SW1_GPIO_Port GPIOA
#define USB_CDC_DM_Pin GPIO_PIN_11
#define USB_CDC_DM_GPIO_Port GPIOA
#define USB_CDC_DP_Pin GPIO_PIN_12
#define USB_CDC_DP_GPIO_Port GPIOA
#define LCD_SCL_Pin GPIO_PIN_15
#define LCD_SCL_GPIO_Port GPIOA
#define USART_USB_TX_Pin GPIO_PIN_10
#define USART_USB_TX_GPIO_Port GPIOC
#define USART_USB_RX_Pin GPIO_PIN_11
#define USART_USB_RX_GPIO_Port GPIOC
#define LCD_SDA_Pin GPIO_PIN_7
#define LCD_SDA_GPIO_Port GPIOB

/* USER CODE BEGIN Private defines */

/* USER CODE END Private defines */

#ifdef __cplusplus
}
#endif

#endif /* __MAIN_H */
