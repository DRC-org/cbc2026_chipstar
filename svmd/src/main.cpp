#include <Arduino.h>
#include <Arduino_CAN.h>
#include <Servo.h>

#include "domain/servo_can_protocol.hpp"
#include "domain/status_led.hpp"
#include <cstring>

namespace proto = domain::servo_can;

namespace {

constexpr uint8_t SERVO_PINS[proto::CHANNEL_COUNT] = {3, 6, 10, 9};

enum class Status : uint8_t { Ok = 0, BadCommand = 1, Timeout = 2 };

Servo servos[proto::CHANNEL_COUNT];
bool enabled[proto::CHANNEL_COUNT] = {};
uint16_t target_us[proto::CHANNEL_COUNT] = {1500, 1500, 1500, 1500};
proto::Parameters parameters;
// 基板アドレスはID用DIPのA0..A1から起動時に読む。
const uint8_t ADDRESS_PINS[] = {A0, A1};
uint8_t address = 0;
uint32_t last_contact_ms = 0;
bool timeout_reported = false;

// svmdはHELLOを持たないので、最初の指令を受けるまでBootとする。
bool contacted = false;

uint8_t enabledMask() {
    uint8_t mask = 0;
    for (uint8_t channel = 0; channel < proto::CHANNEL_COUNT; ++channel) {
        if (enabled[channel]) mask |= static_cast<uint8_t>(1U << channel);
    }
    return mask;
}

domain::Status ledStatus() {
    if (timeout_reported) return domain::Status::Error;
    if (!contacted) return domain::Status::Boot;
    return enabledMask() != 0 ? domain::Status::Run : domain::Status::Safe;
}

void setEnabled(uint8_t channel, bool value) {
    if (enabled[channel] == value) return;
    enabled[channel] = value;
    if (value) {
        servos[channel].attach(SERVO_PINS[channel], parameters.minPulseUs(),
                               parameters.maxPulseUs());
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
    CAN.write(CanMsg(CanStandardId(proto::canId(proto::STATUS_ID, address)), sizeof(data), data));
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
        case proto::CommandKind::ParamSet:
            parameters.set(command.param_id, command.value);
            {
                const float value = parameters.get(static_cast<proto::ParamId>(command.param_id));
                uint32_t bits; std::memcpy(&bits, &value, sizeof(bits));
                const uint8_t report[8] = {1, command.param_id, 0, 0,
                    static_cast<uint8_t>(bits >> 24), static_cast<uint8_t>(bits >> 16),
                    static_cast<uint8_t>(bits >> 8), static_cast<uint8_t>(bits)};
                CAN.write(CanMsg(CanStandardId(proto::canId(0x304, address)), sizeof(report), report));
            }
            break;
        case proto::CommandKind::Invalid:
            return;
    }
    sendStatus(Status::Ok, command);
}
}  // namespace

void setup() {
    pinMode(LED_BUILTIN, OUTPUT);
    for (auto pin : ADDRESS_PINS) pinMode(pin, INPUT_PULLUP);
    for (uint8_t i = 0; i < 2; ++i) {
        if (digitalRead(ADDRESS_PINS[i]) == LOW) address |= 1U << i;
    }
    Serial.begin(115200);
    stopAll();
    CAN.begin(CanBitRate::BR_1000k);
    last_contact_ms = millis();
}

void loop() {
    digitalWrite(LED_BUILTIN,
                 domain::statusPattern(millis(), ledStatus(), 1) ? HIGH : LOW);
    while (CAN.available()) {
        const CanMsg message = CAN.read();
        if (message.id != proto::canId(proto::COMMAND_ID, address)) continue;
        contacted = true;

        proto::Command command;
        if (!proto::parse(message.data, message.data_length, command, parameters)) {
            sendStatus(Status::BadCommand, command);
            continue;
        }

        last_contact_ms = millis();
        timeout_reported = false;
        apply(command);
    }

    if (!timeout_reported && millis() - last_contact_ms > parameters.watchdogMs()) {
        stopAll();
        proto::Command timeout;
        sendStatus(Status::Timeout, timeout);
        timeout_reported = true;
    }
}
