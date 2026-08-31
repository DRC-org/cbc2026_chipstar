#pragma once

#include <cstddef>
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

// 指令生値[±16384] を電流[mA] へ戻す。
int32_t rawToCurrent(int16_t raw);

// 指令フレームは 4 台分をまとめて運ぶ。ESC ID で載るフレームが決まる。
constexpr uint16_t COMMAND_ID_1_TO_4 = 0x200;
constexpr uint16_t COMMAND_ID_5_TO_8 = 0x1FF;
constexpr uint8_t MIN_ESC_ID = 1;
constexpr uint8_t MAX_ESC_ID = 8;
constexpr std::size_t MOTORS_PER_FRAME = 4;

// ESC ID が属する指令フレームの ID。範囲外なら 0 を返す。
uint16_t groupCommandId(uint8_t esc_id);

// 4ch 一括指令フレームのうち、esc_id に対応する 2 バイトへ書き込む。
// ESC ID 1..4 と 5..8 は別フレームだが、フレーム内の位置は同じ並びになる。
void writeCommandSlot(uint8_t frame[8], uint8_t esc_id, int16_t raw);

// 同一グループ最大 4 台分の電流指令をまとめるフレーム。
//
// C620 の指令フレームは 1 本で 4 台分を運ぶ。1 台ごとに別フレームで送ると、
// 後から届いたフレームが他の台の指令を 0 で上書きしてしまう。
// 同じグループの台は必ずこのフレームに集めてから 1 回で送る。
class CommandFrame {
public:
    explicit CommandFrame(uint16_t command_id) : command_id_(command_id) {}

    uint16_t commandId() const { return command_id_; }
    const uint8_t* data() const { return data_; }

    // 全スロットを 0 にする。送信のたびに呼び、書かれなかった台は 0 電流になる。
    void clear();

    // esc_id がこのフレームの担当なら書き込んで true を返す。
    bool set(uint8_t esc_id, int16_t raw);

private:
    uint16_t command_id_;
    uint8_t data_[8] = {};
};

}  // namespace domain::c620
