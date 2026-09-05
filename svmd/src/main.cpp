#include <Arduino.h>
#include <Arduino_CAN.h>
#include <Servo.h>

#include "domain/servo_can_protocol.hpp"
#include "domain/digital_inputs.hpp"

namespace proto = domain::servo_can;

namespace {
constexpr uint32_t COMMAND_CAN_ID = 0x300;
constexpr uint32_t STATUS_CAN_ID = 0x301;
constexpr uint32_t WATCHDOG_MS = 250;
constexpr uint8_t SERVO_PINS[proto::CHANNEL_COUNT] = {3, 6, 10, 9};

enum class Status : uint8_t { Ok = 0, BadCommand = 1, Timeout = 2 };

Servo servos[proto::CHANNEL_COUNT];
bool enabled[proto::CHANNEL_COUNT] = {};
uint16_t target_us[proto::CHANNEL_COUNT] = {1500, 1500, 1500, 1500};
uint32_t last_contact_ms = 0;
bool timeout_reported = false;
domain::DigitalInputs inputs(0);  // 外部SW入力なし。A0..A3は基板ID用DIP。
const uint8_t DIP_PINS[] = {A0, A1, A2, A3};

uint8_t enabledMask() {
    uint8_t mask = 0;
    for (uint8_t channel = 0; channel < proto::CHANNEL_COUNT; ++channel) {
        if (enabled[channel]) mask |= static_cast<uint8_t>(1U << channel);
    }
    return mask;
}

void setEnabled(uint8_t channel, bool value) {
    if (enabled[channel] == value) return;
    enabled[channel] = value;
    if (value) {
        servos[channel].attach(SERVO_PINS[channel], proto::MIN_PULSE_US, proto::MAX_PULSE_US);
        servos[channel].writeMicroseconds(target_us[channel]);
    } else {
        servos[channel].detach();
    }
}

void stopAll() {
    for (uint8_t channel = 0; channel < proto::CHANNEL_COUNT; ++channel) {
        setEnabled(channel, false);
    }
}

void sendStatus(Status status, const proto::Command& command) {
    const uint8_t channel = command.channel < proto::CHANNEL_COUNT ? command.channel : 0;
    const uint16_t pulse = target_us[channel];
    const uint8_t data[8] = {
        proto::PROTOCOL_VERSION,
        static_cast<uint8_t>(status),
        static_cast<uint8_t>(command.kind),
        channel,
        enabledMask(),
        static_cast<uint8_t>(pulse >> 8),
        static_cast<uint8_t>(pulse & 0xFF),
        0,
    };
    CAN.write(CanMsg(CanStandardId(STATUS_CAN_ID), sizeof(data), data));
}

void apply(const proto::Command& command) {
    switch (command.kind) {
        case proto::CommandKind::Stop:
            stopAll();
            break;
        case proto::CommandKind::Set:
            target_us[command.channel] = command.pulse_us;
            if (enabled[command.channel]) {
                servos[command.channel].writeMicroseconds(command.pulse_us);
            }
            break;
        case proto::CommandKind::Enable:
            setEnabled(command.channel, command.enabled);
            break;
        case proto::CommandKind::Heartbeat:
            break;
        case proto::CommandKind::InputRead: {
            uint8_t data[8]; inputs.encode(data);
            CAN.write(CanMsg(CanStandardId(0x302), sizeof(data), data));
            return;
        }
        case proto::CommandKind::Invalid:
            return;
    }
    sendStatus(Status::Ok, command);
}
}  // namespace

void setup() {
    for (auto pin : DIP_PINS) pinMode(pin, INPUT_PULLUP);
    Serial.begin(115200);
    stopAll();
    CAN.begin(CanBitRate::BR_1000k);
    last_contact_ms = millis();
}

void loop() {
    uint8_t dip = 0;
    for (uint8_t i = 0; i < 4; ++i) if (digitalRead(DIP_PINS[i]) == LOW) dip |= 1U << i;
    inputs.sample(0, dip, millis());
    while (CAN.available()) {
        const CanMsg message = CAN.read();
        if (message.id != COMMAND_CAN_ID) continue;

        proto::Command command;
        if (!proto::parse(message.data, message.data_length, command)) {
            sendStatus(Status::BadCommand, command);
            continue;
        }

        last_contact_ms = millis();
        timeout_reported = false;
        apply(command);
    }

    if (!timeout_reported && millis() - last_contact_ms > WATCHDOG_MS) {
        stopAll();
        proto::Command timeout;
        sendStatus(Status::Timeout, timeout);
        timeout_reported = true;
    }
}
