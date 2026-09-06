#pragma once
#include <cstdint>

namespace domain {

// 基板の状態を示すLEDの点け方。
//
// 「このLEDが点いていたら○○」という割当は、どのLEDが何番かを覚えていないと
// 読めず、離れると判別できない。そこで点灯位置ではなく、動き方と速さで表す。
// LEDの数が違う基板でも同じ見え方になるようにしてある。
//
//   Boot  順送り（1灯ずつ流れて、消灯で一拍おく）  hostと確立していない
//   Safe  全灯をゆっくり点滅（1Hz）                待機。出力は無効
//   Run   全灯を常時点灯                           動作中
//   Stop  全灯を速く点滅（4Hz）                    停止した
//   Error 全灯を2回続けて点滅し、休む              異常。原因は通信で確認する
enum class Status : uint8_t {
    Boot,
    Safe,
    Run,
    Stop,
    Error,
};

// 点灯させるLEDのbit mask。bit 0 が1番目のLED。
inline uint8_t statusPattern(uint32_t tick_ms, Status status, uint8_t led_count) {
    if (led_count == 0) return 0;
    const uint8_t all =
        led_count >= 8 ? 0xFF : static_cast<uint8_t>((1U << led_count) - 1);
    switch (status) {
        case Status::Run:
            return all;
        case Status::Safe:
            return (tick_ms % 1000) < 500 ? all : 0;
        case Status::Stop:
            return (tick_ms % 250) < 125 ? all : 0;
        case Status::Error: {
            // 短く2回光って休む。速い点滅とはリズムで区別できる。
            const uint32_t phase = tick_ms % 1000;
            const bool lit = phase < 100 || (phase >= 200 && phase < 300);
            return lit ? all : 0;
        }
        case Status::Boot: {
            // 1灯ずつ流し、末尾に消灯を3コマ入れて向きを分かるようにする。
            const uint32_t steps = static_cast<uint32_t>(led_count) + 3;
            const uint32_t step = (tick_ms / 100) % steps;
            return step < led_count ? static_cast<uint8_t>(1U << step) : 0;
        }
    }
    return 0;
}

}  // namespace domain
