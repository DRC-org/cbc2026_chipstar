#include "domain/command.hpp"

#include <cmath>
#include <cstdlib>

namespace domain {
namespace {

bool isSpace(char ch) { return ch == ' ' || ch == '\t'; }

char toUpper(char ch) {
    return (ch >= 'a' && ch <= 'z') ? static_cast<char>(ch - 'a' + 'A') : ch;
}

constexpr std::size_t MAX_TOKENS = 4;

struct Token {
    const char* begin = nullptr;
    std::size_t length = 0;
};

std::size_t tokenize(const char* line, std::size_t length, Token* tokens) {
    std::size_t count = 0;
    std::size_t i = 0;
    while (i < length && count <= MAX_TOKENS) {
        while (i < length && isSpace(line[i])) ++i;
        if (i >= length) break;
        const std::size_t start = i;
        while (i < length && !isSpace(line[i])) ++i;
        if (count == MAX_TOKENS) return MAX_TOKENS + 1;
        tokens[count++] = Token{&line[start], i - start};
    }
    return count;
}

bool equalsIgnoreCase(const Token& token, const char* keyword) {
    std::size_t i = 0;
    for (; i < token.length; ++i) {
        if (keyword[i] == '\0' || toUpper(token.begin[i]) != keyword[i]) return false;
    }
    return keyword[i] == '\0';
}

bool parseU8(const Token& token, uint8_t minimum, uint8_t maximum, uint8_t& value) {
    if (token.length == 0 || token.length > 3) return false;
    uint16_t parsed = 0;
    for (std::size_t i = 0; i < token.length; ++i) {
        if (token.begin[i] < '0' || token.begin[i] > '9') return false;
        parsed = static_cast<uint16_t>(parsed * 10 + token.begin[i] - '0');
    }
    if (parsed < minimum || parsed > maximum) return false;
    value = static_cast<uint8_t>(parsed);
    return true;
}

bool parseU16(const Token& token, uint16_t maximum, uint16_t& value) {
    if (token.length == 0 || token.length > 5) return false;
    uint32_t parsed = 0;
    for (std::size_t i = 0; i < token.length; ++i) {
        if (token.begin[i] < '0' || token.begin[i] > '9') return false;
        parsed = parsed * 10 + static_cast<uint32_t>(token.begin[i] - '0');
    }
    if (parsed > maximum) return false;
    value = static_cast<uint16_t>(parsed);
    return true;
}

bool parseHexNibble(char ch, uint8_t& value) {
    ch = toUpper(ch);
    if (ch >= '0' && ch <= '9') {
        value = static_cast<uint8_t>(ch - '0');
        return true;
    }
    if (ch >= 'A' && ch <= 'F') {
        value = static_cast<uint8_t>(ch - 'A' + 10);
        return true;
    }
    return false;
}

bool parseCanData(const Token& token, uint8_t* data, uint8_t& length) {
    if (token.length == 1 && token.begin[0] == '-') {
        length = 0;
        return true;
    }
    if (token.length == 0 || token.length > 16 || token.length % 2 != 0) return false;
    length = static_cast<uint8_t>(token.length / 2);
    for (uint8_t i = 0; i < length; ++i) {
        uint8_t high = 0;
        uint8_t low = 0;
        if (!parseHexNibble(token.begin[i * 2], high) ||
            !parseHexNibble(token.begin[i * 2 + 1], low)) {
            return false;
        }
        data[i] = static_cast<uint8_t>((high << 4) | low);
    }
    return true;
}

bool parseFlag(const Token& token, bool& value) {
    uint8_t parsed = 0;
    if (!parseU8(token, 0, 1, parsed)) return false;
    value = parsed != 0;
    return true;
}

bool parseFloat(const Token& token, float& value) {
    if (token.length == 0 || token.length >= 24) return false;
    char buffer[24] = {};
    for (std::size_t i = 0; i < token.length; ++i) buffer[i] = token.begin[i];
    char* end = nullptr;
    value = std::strtof(buffer, &end);
    return end == &buffer[token.length] && std::isfinite(value);
}

}  // namespace

Command parseCommand(const char* line, std::size_t length) {
    Command command;
    if (line == nullptr) return command;

    Token tokens[MAX_TOKENS];
    const std::size_t count = tokenize(line, length, tokens);
    if (count == 0 || count > MAX_TOKENS) return command;

    if (count == 1) {
        if (equalsIgnoreCase(tokens[0], "STOP")) command.kind = CommandKind::Stop;
        else if (equalsIgnoreCase(tokens[0], "RUN")) command.kind = CommandKind::Run;
        else if (equalsIgnoreCase(tokens[0], "SAFE")) command.kind = CommandKind::Safe;
        else if (equalsIgnoreCase(tokens[0], "HEARTBEAT")) command.kind = CommandKind::Heartbeat;
        return command;
    }

    if (count == 2 && equalsIgnoreCase(tokens[0], "HELLO")) {
        if (parseU8(tokens[1], 1, 255, command.protocol_version)) command.kind = CommandKind::Hello;
        return command;
    }
    if (count == 2 && equalsIgnoreCase(tokens[0], "HOME")) {
        if (parseU8(tokens[1], 1, slot_bit::ALL, command.mask)) command.kind = CommandKind::Home;
        return command;
    }
    if (count == 3 && equalsIgnoreCase(tokens[0], "ENABLE")) {
        if (parseU8(tokens[1], 1, slot_bit::ALL, command.mask) &&
            parseFlag(tokens[2], command.value)) {
            command.kind = CommandKind::Enable;
        }
        return command;
    }
    if (count == 3 && equalsIgnoreCase(tokens[0], "TARGET")) {
        if (parseU8(tokens[1], 0, SLOT_COUNT - 1, command.slot) &&
            parseFloat(tokens[2], command.target)) {
            command.kind = CommandKind::Target;
        }
        return command;
    }
    if (count == 4 && equalsIgnoreCase(tokens[0], "CAN")) {
        uint8_t bus = 0;
        if (parseU8(tokens[1], 2, 2, bus) && parseU16(tokens[2], 0x7FF, command.can_id) &&
            parseCanData(tokens[3], command.can_data, command.can_length)) {
            command.kind = CommandKind::CanTx;
        }
        return command;
    }
    return command;
}

}  // namespace domain
