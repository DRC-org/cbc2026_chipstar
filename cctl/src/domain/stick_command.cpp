#include "domain/stick_command.hpp"

namespace domain {
namespace {
// 最短の有効な行 "LX+000 LY+000" の長さ。
constexpr std::size_t MIN_LINE_LENGTH = 13;

// "+000" 形式の符号付き 3 桁を読む。
bool parsePercent(const char* text, int8_t& value) {
    if ((text[0] != '+' && text[0] != '-') ||
        text[1] < '0' || text[1] > '9' ||
        text[2] < '0' || text[2] > '9' ||
        text[3] < '0' || text[3] > '9') {
        return false;
    }

    int parsed = (text[1] - '0') * 100 + (text[2] - '0') * 10 + (text[3] - '0');
    if (text[0] == '-') {
        parsed = -parsed;
    }
    if (parsed < -100 || parsed > 100) {
        return false;
    }

    value = static_cast<int8_t>(parsed);
    return true;
}
}  // namespace

bool parseStickCommand(const char* line, std::size_t length, StickCommand& out) {
    if (line == nullptr || length < MIN_LINE_LENGTH || line[0] != 'L' || line[1] != 'X' ||
        line[6] != ' ' || line[7] != 'L' || line[8] != 'Y') {
        return false;
    }

    int8_t lx = 0;
    int8_t ly = 0;
    if (!parsePercent(&line[2], lx) || !parsePercent(&line[9], ly)) {
        return false;
    }

    out.lx_percent = lx;
    out.ly_percent = ly;
    return true;
}

float applyDeadzone(float value, float deadzone) {
    const float magnitude = value < 0.0f ? -value : value;
    if (magnitude <= deadzone) {
        return 0.0f;
    }

    const float scaled = (magnitude - deadzone) / (1.0f - deadzone);
    return value < 0.0f ? -scaled : scaled;
}

ManualDelta computeManualDelta(const StickCommand& command, float interval_s, float deadzone,
                               float r_speed_mm_s, float theta_speed_deg_s) {
    const float lx = static_cast<float>(command.lx_percent) / 100.0f;
    const float ly = static_cast<float>(command.ly_percent) / 100.0f;

    ManualDelta delta = {};
    delta.r_mm = applyDeadzone(ly, deadzone) * r_speed_mm_s * interval_s;
    delta.theta_deg = applyDeadzone(lx, deadzone) * theta_speed_deg_s * interval_s;
    return delta;
}

}  // namespace domain
