#include "doctest.h"

#include "domain/delta_timer.hpp"

using domain::DeltaTimer;

TEST_CASE("初回は経過時間を 0 とする") {
    // 前回の呼び出しが無い時点では経過時間が決まらない。
    DeltaTimer timer;
    CHECK(timer.update(1000) == doctest::Approx(0.0f));
}

TEST_CASE("2 回目以降はミリ秒差を秒で返す") {
    DeltaTimer timer;
    timer.update(1000);

    CHECK(timer.update(1100) == doctest::Approx(0.1f));
    CHECK(timer.update(1101) == doctest::Approx(0.001f));
}

TEST_CASE("同じ tick なら 0") {
    DeltaTimer timer;
    timer.update(500);

    CHECK(timer.update(500) == doctest::Approx(0.0f));
}

TEST_CASE("tick の 32 ビット巻き戻りを跨げる") {
    // HAL_GetTick() は約 49.7 日で 0 に戻る。
    DeltaTimer timer;
    timer.update(0xFFFFFFFF);

    CHECK(timer.update(9) == doctest::Approx(0.010f));
}

TEST_CASE("reset すると次回が初回扱いになる") {
    DeltaTimer timer;
    timer.update(1000);
    timer.reset();

    CHECK(timer.update(5000) == doctest::Approx(0.0f));
}
