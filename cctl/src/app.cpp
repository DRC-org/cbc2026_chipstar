#include "lcd_aqm1602.h"
#include "main.h"

#include <cstddef>
#include <cstdint>
#include <cstring>

extern "C" {
extern FDCAN_HandleTypeDef hfdcan1;
extern FDCAN_HandleTypeDef hfdcan2;
extern FDCAN_HandleTypeDef hfdcan3;
extern I2C_HandleTypeDef hi2c1;
extern I2C_HandleTypeDef hi2c3;
extern I2C_HandleTypeDef hi2c4;
extern TIM_HandleTypeDef htim15;
extern UART_HandleTypeDef huart3;
extern PCD_HandleTypeDef hpcd_USB_FS;
extern TIM_HandleTypeDef htim2;
}

namespace {
Aqm1602 lcd(&hi2c1);

constexpr std::size_t kLcdColumns = 16;
constexpr std::size_t kControllerLineMax = 96;

char rx_line[kControllerLineMax] = {};
std::size_t rx_line_length = 0;
char pending_controller_line[kControllerLineMax] = {};
volatile bool controller_line_ready = false;

void playTone(uint32_t frequency_hz, uint32_t duration_ms) {
    const uint32_t period = (1000000U / frequency_hz) - 1U;

    __HAL_TIM_SET_AUTORELOAD(&htim15, period);
    __HAL_TIM_SET_COMPARE(&htim15, TIM_CHANNEL_2, (period + 1U) / 2U);

    HAL_TIM_PWM_Start(&htim15, TIM_CHANNEL_2);
    HAL_Delay(duration_ms);
    HAL_TIM_PWM_Stop(&htim15, TIM_CHANNEL_2);
}

void printLcdSegment(const char *text, std::size_t offset) {
    char line[kLcdColumns + 1] = {};
    std::memset(line, ' ', kLcdColumns);
    line[kLcdColumns] = '\0';

    const std::size_t length = std::strlen(text);
    if (offset < length) {
        const std::size_t copy_length = (length - offset) < kLcdColumns ? (length - offset) : kLcdColumns;
        std::memcpy(line, text + offset, copy_length);
    }

    lcd.print(line);
}

void showStartupLcd() {
    lcd.clear();
    lcd.setCursor(0, 0);
    lcd.print("DRC-CCTL2026");
    lcd.setCursor(0, 1);
    lcd.print("Controller input");
}

void showControllerInput(const char *text) {
    lcd.clear();
    lcd.setCursor(0, 0);
    printLcdSegment(text, 0);
    lcd.setCursor(0, 1);
    printLcdSegment(text, kLcdColumns);
}

void commitControllerLine() {
    rx_line[rx_line_length] = '\0';

    __disable_irq();
    std::strncpy(pending_controller_line, rx_line, sizeof(pending_controller_line) - 1);
    pending_controller_line[sizeof(pending_controller_line) - 1] = '\0';
    controller_line_ready = true;
    __enable_irq();

    rx_line_length = 0;
}

void acceptControllerByte(uint8_t value) {
    if (value == '\r') {
        return;
    }

    if (value == '\n') {
        if (rx_line_length > 0) {
            commitControllerLine();
        }
        return;
    }

    if (value < 0x20 || value > 0x7e) {
        return;
    }

    if (rx_line_length < sizeof(rx_line) - 1) {
        rx_line[rx_line_length++] = static_cast<char>(value);
    }
}

void updateLedPattern(uint32_t tick_ms) {
    const auto cycle_ms = tick_ms % 300U;
    const bool led1_on = cycle_ms < 100U || cycle_ms >= 285U;
    const bool led2_on = cycle_ms >= 85U && cycle_ms < 200U;
    const bool led3_on = cycle_ms >= 185U && cycle_ms < 300U;

    HAL_GPIO_WritePin(LED1_GPIO_Port, LED1_Pin, led1_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
    HAL_GPIO_WritePin(LED2_GPIO_Port, LED2_Pin, led2_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
    HAL_GPIO_WritePin(LED3_GPIO_Port, LED3_Pin, led3_on ? GPIO_PIN_SET : GPIO_PIN_RESET);
}
} // namespace

extern "C" void setup(void) {
    lcd.begin();
    showStartupLcd();

    playTone(988, 80);
    playTone(1319, 120);

    HAL_TIM_Base_Start_IT(&htim2);
}

extern "C" void loop(void) {
    char controller_line[kControllerLineMax] = {};
    bool update_lcd = false;

    uint8_t rx;
    if (HAL_UART_Receive(&huart3, &rx, 1, 0) == HAL_OK) {
        HAL_UART_Transmit(&huart3, &rx, 1, 10);
        acceptControllerByte(rx);
    }

    __disable_irq();
    if (controller_line_ready) {
        controller_line_ready = false;
        std::strncpy(controller_line, pending_controller_line, sizeof(controller_line) - 1);
        update_lcd = true;
    }
    __enable_irq();

    if (update_lcd) {
        showControllerInput(controller_line);
    }

    HAL_Delay(1);
}

extern "C" void cctl_usbcdc_receive(const uint8_t *data, uint32_t length) {
    if (data == nullptr) {
        return;
    }

    for (uint32_t i = 0; i < length; ++i) {
        acceptControllerByte(data[i]);
    }
}

extern "C" void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef *htim) {
    if (htim->Instance == TIM2) {
        static uint32_t tick_ms = 0;

        tick_ms++;
        if (tick_ms % 100U == 0U) {
            updateLedPattern(tick_ms);
        }
    }
}
