#include "doctest.h"

#include "domain/kinematics.hpp"

#include <initializer_list>

using namespace domain::kinematics;

TEST_CASE("1 回転あたりの送り量で mm と rad を換算する") {
    // 1 回転 = 40mm のベルトなら、40mm 進むのに 1 回転。
    CHECK(mmToRad(40.0f, 40.0f, 1.0f) == doctest::Approx(6.28318f).epsilon(0.0001));
    CHECK(mmToRad(20.0f, 40.0f, 1.0f) == doctest::Approx(3.14159f).epsilon(0.0001));
    CHECK(mmToRad(0.0f, 40.0f, 1.0f) == doctest::Approx(0.0f));
}

TEST_CASE("符号で方向を反転できる") {
    CHECK(mmToRad(40.0f, 40.0f, -1.0f) == doctest::Approx(-6.28318f).epsilon(0.0001));
}

TEST_CASE("mm と rad は往復しても値が戻る") {
    // テレメトリで実測 rad を mm に戻すため、逆変換が一致している必要がある。
    for (const float sign : {1.0f, -1.0f}) {
        for (const float mm : {0.0f, 12.5f, -37.25f, 250.0f}) {
            const float rad = mmToRad(mm, 62.8f, sign);
            CHECK(radToMm(rad, 62.8f, sign) == doctest::Approx(mm).epsilon(0.0001));
        }
    }
}

TEST_CASE("出力軸角とモータ軸角は減速比で換算する") {
    CHECK(outputDegToMotorDeg(10.0f, 140.0f, 1.0f) == doctest::Approx(1400.0f));
    CHECK(motorDegToOutputDeg(1400.0f, 140.0f, 1.0f) == doctest::Approx(10.0f));
}

TEST_CASE("出力軸角も往復して値が戻る") {
    for (const float sign : {1.0f, -1.0f}) {
        for (const float deg : {0.0f, 45.0f, -180.0f}) {
            const float motor = outputDegToMotorDeg(deg, 140.823f, sign);
            CHECK(motorDegToOutputDeg(motor, 140.823f, sign) ==
                  doctest::Approx(deg).epsilon(0.0001));
        }
    }
}

TEST_CASE("換算係数が 0 なら 0 を返す") {
    // 実測前の設定漏れで NaN を撒くより、動かないほうが原因を追える。
    CHECK(mmToRad(100.0f, 0.0f, 1.0f) == doctest::Approx(0.0f));
    CHECK(motorDegToOutputDeg(100.0f, 0.0f, 1.0f) == doctest::Approx(0.0f));
}
