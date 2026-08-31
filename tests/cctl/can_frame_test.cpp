#include "doctest.h"

#include "domain/can_frame.hpp"

using domain::Axis;
using domain::CanFrame;
using domain::classifyFeedback;

namespace {
constexpr uint16_t THETA_ID = 0x201;
constexpr uint16_t Z_ID = 0x00A;

CanFrame standard(uint32_t id) {
    CanFrame frame;
    frame.id = id;
    frame.extended = false;
    frame.length = 8;
    return frame;
}

CanFrame extended(uint32_t id) {
    CanFrame frame;
    frame.id = id;
    frame.extended = true;
    frame.length = 8;
    return frame;
}
}  // namespace

TEST_CASE("拡張IDは r 軸のフィードバック") {
    // EL05 だけが拡張IDを使うので、ID の値を見るまでもなく r で確定する。
    CHECK(classifyFeedback(extended(0x12345678), THETA_ID, Z_ID) == Axis::R);
    CHECK(classifyFeedback(extended(0x201), THETA_ID, Z_ID) == Axis::R);
}

TEST_CASE("標準IDは設定値との一致で振り分ける") {
    CHECK(classifyFeedback(standard(THETA_ID), THETA_ID, Z_ID) == Axis::Theta);
    CHECK(classifyFeedback(standard(Z_ID), THETA_ID, Z_ID) == Axis::Z);
}

TEST_CASE("どれにも一致しない標準IDは無視する") {
    // 同じバスに周辺基板が乗っても、知らない ID は捨てる。
    CHECK(classifyFeedback(standard(0x123), THETA_ID, Z_ID) == Axis::None);
    CHECK(classifyFeedback(standard(0x200), THETA_ID, Z_ID) == Axis::None);
}

TEST_CASE("θ と z に同じ ID を設定した場合は θ が優先される") {
    // 設定ミスの挙動を固定しておく。
    CHECK(classifyFeedback(standard(0x201), 0x201, 0x201) == Axis::Theta);
}
