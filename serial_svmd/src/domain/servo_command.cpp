#include "domain/servo_command.hpp"

#include "device_config.hpp"

namespace domain {
namespace {
constexpr std::size_t MAX_TOKENS = 6;

struct Token {
    const char* begin = nullptr;
    std::size_t length = 0;
};

bool space(char ch) { return ch == ' ' || ch == '\t'; }
char upper(char ch) {
    return ch >= 'a' && ch <= 'z' ? static_cast<char>(ch - 'a' + 'A') : ch;
}

std::size_t tokenize(const char* line, std::size_t length, Token* tokens) {
    std::size_t count = 0;
    std::size_t i = 0;
    while (i < length && count <= MAX_TOKENS) {
        while (i < length && space(line[i])) ++i;
        if (i >= length) break;
        const std::size_t start = i;
        while (i < length && !space(line[i])) ++i;
        if (count == MAX_TOKENS) return MAX_TOKENS + 1;
        tokens[count++] = Token{&line[start], i - start};
    }
    return count;
}

bool equal(const Token& token, const char* word) {
    std::size_t i = 0;
    for (; i < token.length; ++i) {
        if (word[i] == '\0' || upper(token.begin[i]) != word[i]) return false;
    }
    return word[i] == '\0';
}

bool number(const Token& token, uint32_t maximum, uint32_t& value) {
    if (token.length == 0 || token.length > 10) return false;
    uint32_t parsed = 0;
    for (std::size_t i = 0; i < token.length; ++i) {
        if (token.begin[i] < '0' || token.begin[i] > '9') return false;
        const uint32_t digit = static_cast<uint32_t>(token.begin[i] - '0');
        if (digit > maximum || parsed > (maximum - digit) / 10) return false;
        parsed = parsed * 10 + digit;
    }
    value = parsed;
    return true;
}

bool id(const Token& token, uint8_t& value) {
    uint32_t parsed = 0;
    if (!number(token, 253, parsed) || parsed == 0) return false;
    value = static_cast<uint8_t>(parsed);
    return true;
}
}  // namespace

ServoCommand parseServoCommand(const char* line, std::size_t length) {
    ServoCommand command;
    if (line == nullptr) return command;
    Token tokens[MAX_TOKENS];
    const std::size_t count = tokenize(line, length, tokens);
    if (count == 0 || count > MAX_TOKENS) return command;

    if (count == 1) {
        if (equal(tokens[0], "SAFE")) command.kind = ServoCommandKind::Safe;
        else if (equal(tokens[0], "RUN")) command.kind = ServoCommandKind::Run;
        else if (equal(tokens[0], "STOP")) command.kind = ServoCommandKind::Stop;
        else if (equal(tokens[0], "HEARTBEAT")) command.kind = ServoCommandKind::Heartbeat;
        return command;
    }
    uint32_t value = 0;
    if (count == 2 && equal(tokens[0], "INPUT") && equal(tokens[1], "READ")) {
        command.kind = ServoCommandKind::InputRead;
        return command;
    }
    if (count == 4 && equal(tokens[0], "INPUT") && equal(tokens[1], "GUARD")) {
        uint32_t high = 0;
        if (number(tokens[2], 63, value) && number(tokens[3], 63, high) && !(high & ~value)) {
            command.kind = ServoCommandKind::InputGuard;
            command.input_mask = static_cast<uint8_t>(value);
            command.input_high = static_cast<uint8_t>(high);
        }
        return command;
    }
    if (count == 2 && equal(tokens[0], "HELLO") && number(tokens[1], 255, value) && value > 0) {
        command.kind = ServoCommandKind::Hello;
        command.protocol_version = static_cast<uint8_t>(value);
        return command;
    }
    if (!equal(tokens[0], "SERVO") || count < 3 || !id(tokens[2], command.id)) return command;

    if (count == 3 && equal(tokens[1], "READ")) {
        command.kind = ServoCommandKind::Read;
    } else if (count == 4 && equal(tokens[1], "ENABLE") && number(tokens[3], 1, value)) {
        command.kind = ServoCommandKind::Enable;
        command.enabled = value != 0;
    } else if (count == 6 && equal(tokens[1], "TARGET") && number(tokens[3], 4095, value)) {
        command.position = static_cast<uint16_t>(value);
        if (!number(tokens[4], config::MAX_SPEED, value)) return ServoCommand{};
        command.speed = static_cast<uint16_t>(value);
        if (!number(tokens[5], config::MAX_ACCELERATION, value)) return ServoCommand{};
        command.acceleration = static_cast<uint8_t>(value);
        command.kind = ServoCommandKind::Target;
    }
    return command;
}

}  // namespace domain
