#include "doctest.h"

#include "domain/pid.hpp"

using domain::Pid;

TEST_CASE("dt が 0 なら P 項だけが出る") {
    // 初回呼び出しは経過時間が不明なので、積分・微分を進めてはいけない。
    Pid pid(2.0f, 100.0f, 100.0f, 1000.0f);

    CHECK(pid.update(1.0f, 0.0f, 0.0f) == doctest::Approx(2.0f));
}

TEST_CASE("積分項は誤差と時間の積を溜める") {
    Pid pid(0.0f, 1.0f, 0.0f, 100.0f);

    CHECK(pid.update(1.0f, 0.0f, 0.5f) == doctest::Approx(0.5f));
    CHECK(pid.update(1.0f, 0.0f, 0.5f) == doctest::Approx(1.0f));
}

TEST_CASE("微分項は誤差の変化率に比例する") {
    Pid pid(0.0f, 0.0f, 2.0f, 100.0f);

    // 1 回目で誤差 0 を記録し、2 回目で 0 → 1 の変化を見る。
    pid.update(0.0f, 0.0f, 0.0f);
    CHECK(pid.update(1.0f, 0.0f, 0.1f) == doctest::Approx(20.0f));
    // 誤差が変わらなければ微分は 0。
    CHECK(pid.update(1.0f, 0.0f, 0.1f) == doctest::Approx(0.0f));
}

TEST_CASE("出力は上下限にクランプされる") {
    Pid pid(100.0f, 0.0f, 0.0f, 5.0f);

    CHECK(pid.update(1.0f, 0.0f, 0.1f) == doctest::Approx(5.0f));
    CHECK(pid.update(-1.0f, 0.0f, 0.1f) == doctest::Approx(-5.0f));
}

TEST_CASE("飽和して誤差が同方向なら積分を捨てる") {
    // anti-windup。飽和中に積分が伸び続けると復帰時に大きく行き過ぎる。
    Pid pid(0.0f, 10.0f, 0.0f, 1.0f);

    CHECK(pid.update(1.0f, 0.0f, 1.0f) == doctest::Approx(1.0f));

    // 積分が捨てられているので、目標に追いついた直後の出力は 0 に戻る。
    CHECK(pid.update(0.0f, 0.0f, 1.0f) == doctest::Approx(0.0f));
}

TEST_CASE("飽和していなければ積分は保持される") {
    Pid pid(0.0f, 1.0f, 0.0f, 100.0f);

    pid.update(1.0f, 0.0f, 1.0f);
    // 誤差が 0 になっても、溜めた積分は残る。
    CHECK(pid.update(0.0f, 0.0f, 1.0f) == doctest::Approx(1.0f));
}

TEST_CASE("reset で内部状態が消える") {
    Pid pid(0.0f, 1.0f, 0.0f, 100.0f);
    pid.update(1.0f, 0.0f, 1.0f);

    pid.reset();

    // reset していなければ積分が 2.0 まで伸びている。
    CHECK(pid.update(1.0f, 0.0f, 1.0f) == doctest::Approx(1.0f));
}

TEST_CASE("ゲインは後から差し替えられる") {
    Pid pid(1.0f, 0.0f, 0.0f, 100.0f);
    pid.setGains(3.0f, 0.0f, 0.0f);

    CHECK(pid.update(1.0f, 0.0f, 0.0f) == doctest::Approx(3.0f));
}
