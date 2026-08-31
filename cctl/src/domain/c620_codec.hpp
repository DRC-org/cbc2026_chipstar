#pragma once

#include <cstdint>

// C620 (M3508 用 ESC) のフレーム符号化・復号と、多回転角の追跡。
// HAL には依存しないので、ホスト側で単体テストできる。
namespace domain::c620 {

// C620 の角度分解能。生角度は 0..8191 で 1 回転する。
constexpr int32_t COUNTS_PER_REV = 8192;

// C620 が返すフィードバックフレームの内容。
struct Feedback {
    uint16_t raw_angle;    // 0..8191
    int16_t rpm;
    int16_t current_raw;   // 電流の生値
    uint8_t temperature;   // [degC]
};

Feedback decodeFeedback(const uint8_t data[8]);

// 0..8191 で巻き戻る生角度から、多回転の累積カウントを追う。
// 最初に渡された角度を原点(0)に採用する（起動位置=原点方針）。
class MultiTurnCounter {
public:
    void update(uint16_t raw_angle);

    // 原点を取り直す。次に受け取った角度が新しい 0 になる。
    void reset();

    bool hasReference() const { return has_reference_; }
    int32_t counts() const { return counts_; }
    float degrees() const;

private:
    bool has_reference_ = false;
    uint16_t last_raw_angle_ = 0;
    int32_t counts_ = 0;
};

// 電流[mA] を C620 の指令生値へ変換する（±20000mA ↔ ±16384）。
int16_t currentToRaw(int32_t milli_amp);

// 4ch 一括指令フレームのうち、esc_id (1..4) に対応する 2 バイトへ書き込む。
void writeCommandSlot(uint8_t frame[8], uint8_t esc_id, int16_t raw);

}  // namespace domain::c620
