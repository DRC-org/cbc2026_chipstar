#include "domain/command.hpp"

namespace domain {
namespace {

bool isSpace(char ch) { return ch == ' ' || ch == '\t'; }

char toUpper(char ch) {
    return (ch >= 'a' && ch <= 'z') ? static_cast<char>(ch - 'a' + 'A') : ch;
}

// 行を空白区切りのトークンに割る。
constexpr std::size_t MAX_TOKENS = 4;

struct Token {
    const char* begin = nullptr;
    std::size_t length = 0;
};

std::size_t tokenize(const char* line, std::size_t length, Token* tokens) {
    std::size_t count = 0;
    std::size_t i = 0;

    while (i < length && count <= MAX_TOKENS) {
        while (i < length && isSpace(line[i])) {
            ++i;
        }
        if (i >= length) {
            break;
        }

        const std::size_t start = i;
        while (i < length && !isSpace(line[i])) {
            ++i;
        }

        if (count == MAX_TOKENS) {
            // 上限を超えた分は数えるだけにして、呼び出し側で弾く。
            return MAX_TOKENS + 1;
        }
        tokens[count].begin = &line[start];
        tokens[count].length = i - start;
        ++count;
    }

    return count;
}

bool equalsIgnoreCase(const Token& token, const char* keyword) {
    std::size_t i = 0;
    for (; i < token.length; ++i) {
        if (keyword[i] == '\0' || toUpper(token.begin[i]) != keyword[i]) {
            return false;
        }
    }
    return keyword[i] == '\0';
}

// "0" / "1" を bool に読む。
bool parseFlag(const Token& token, bool& value) {
    if (token.length != 1 || (token.begin[0] != '0' && token.begin[0] != '1')) {
        return false;
    }
    value = token.begin[0] == '1';
    return true;
}

// "RTZ" のような軸指定をビットマスクに読む。重複は認めない。
bool parseAxes(const Token& token, uint8_t& axes) {
    if (token.length == 0 || token.length > 3) {
        return false;
    }

    uint8_t bits = 0;
    for (std::size_t i = 0; i < token.length; ++i) {
        uint8_t bit = 0;
        switch (toUpper(token.begin[i])) {
            case 'R': bit = axis_bit::R; break;
            case 'T': bit = axis_bit::THETA; break;
            case 'Z': bit = axis_bit::Z; break;
            default: return false;
        }
        if ((bits & bit) != 0) {
            return false;
        }
        bits |= bit;
    }

    axes = bits;
    return true;
}

}  // namespace

Command parseCommand(const char* line, std::size_t length) {
    Command command;
    if (line == nullptr) {
        return command;
    }

    Token tokens[MAX_TOKENS];
    const std::size_t count = tokenize(line, length, tokens);
    if (count == 0 || count > MAX_TOKENS) {
        return command;
    }

    if (count == 1) {
        if (equalsIgnoreCase(tokens[0], "STOP")) {
            command.kind = CommandKind::Stop;
        } else if (equalsIgnoreCase(tokens[0], "RUN")) {
            command.kind = CommandKind::Run;
        } else if (equalsIgnoreCase(tokens[0], "SAFE")) {
            command.kind = CommandKind::Safe;
        } else if (equalsIgnoreCase(tokens[0], "HOME")) {
            command.kind = CommandKind::Home;
        }
        return command;
    }

    if (count == 2 && equalsIgnoreCase(tokens[0], "TEST")) {
        if (parseFlag(tokens[1], command.value)) {
            command.kind = CommandKind::Test;
        }
        return command;
    }

    if (count == 3 && equalsIgnoreCase(tokens[0], "EN")) {
        if (parseAxes(tokens[1], command.axes) && parseFlag(tokens[2], command.value)) {
            command.kind = CommandKind::Enable;
        } else {
            command.axes = 0;
        }
        return command;
    }

    return command;
}

}  // namespace domain
