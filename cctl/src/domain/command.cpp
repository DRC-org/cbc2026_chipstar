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
    return command;
}

}  // namespace domain
