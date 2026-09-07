#include "doctest.h"

#include "domain/feedback_watch.hpp"

using domain::FeedbackWatch;

TEST_CASE("有効になった時点から数え始める") {
    FeedbackWatch watch;
    watch.reset(0);
    // 起動から長く経ってから有効化しても、その場で途絶と判定しない。
    CHECK(watch.update(100000, 0, 200) == 0);
    CHECK(watch.update(100000, 0b001, 200) == 0);
    CHECK(watch.stale() == 0);
}

TEST_CASE("応答が途絶えたslotだけ落とす") {
    FeedbackWatch watch;
    watch.reset(0);
    CHECK(watch.update(0, 0b011, 200) == 0);

    watch.markSeen(0, 100);
    watch.markSeen(1, 100);
    CHECK(watch.update(250, 0b011, 200) == 0);

    // slot0だけ応答が続き、slot1が止まる。
    watch.markSeen(0, 300);
    CHECK(watch.update(350, 0b011, 200) == 0b010);
    CHECK(watch.stale() == 0b010);
}

TEST_CASE("同じ途絶を繰り返し報告しない") {
    FeedbackWatch watch;
    watch.reset(0);
    watch.update(0, 0b001, 200);
    CHECK(watch.update(500, 0b001, 200) == 0b001);
    // 2回目以降は落とす指示を返さない。hostが再有効化するまで待つ。
    CHECK(watch.update(600, 0b001, 200) == 0);
    CHECK(watch.stale() == 0b001);
}

TEST_CASE("応答が戻れば印を消す") {
    FeedbackWatch watch;
    watch.reset(0);
    watch.update(500, 0b001, 200);
    CHECK(watch.stale() == 0b001);
    watch.markSeen(0, 600);
    CHECK(watch.update(650, 0b001, 200) == 0);
    CHECK(watch.stale() == 0);
}

TEST_CASE("無効なslotは途絶にしない") {
    FeedbackWatch watch;
    watch.reset(0);
    CHECK(watch.update(5000, 0, 200) == 0);
    CHECK(watch.stale() == 0);
}

TEST_CASE("タイマの周回をまたいでも判定できる") {
    FeedbackWatch watch;
    watch.reset(UINT32_MAX - 100);
    watch.markSeen(0, UINT32_MAX - 100);
    CHECK(watch.update(UINT32_MAX - 10, 0b001, 200) == 0);
    CHECK(watch.update(150, 0b001, 200) == 0b001);
}

TEST_CASE("応答切れによる出力解除後も原因を保持し実受信で解除する") {
    FeedbackWatch watch;
    watch.reset(0);
    CHECK(watch.update(201, 1, 200) == 1);
    CHECK(watch.update(202, 0, 200) == 0);
    CHECK(watch.stale() == 1);
    CHECK(watch.update(1000, 0, 200) == 0);
    CHECK(watch.stale() == 1);
    watch.markSeen(0, 1001);
    CHECK(watch.update(1002, 0, 200) == 0);
    CHECK(watch.stale() == 0);
}

TEST_CASE("再有効化だけでは途絶履歴を消さず再度の期限切れでも出力を落とす") {
    FeedbackWatch watch;
    watch.reset(0);
    CHECK(watch.update(201, 1, 200) == 1);
    watch.update(1000, 0, 200);
    CHECK(watch.update(1001, 1, 200) == 0);
    CHECK(watch.stale() == 1);
    CHECK(watch.update(1201, 1, 200) == 1);
    CHECK(watch.stale() == 1);
    watch.markSeen(0, 1202);
    CHECK(watch.stale() == 0);
}
