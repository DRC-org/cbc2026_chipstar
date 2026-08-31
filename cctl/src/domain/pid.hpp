#pragma once

namespace domain {

// 汎用 PID 制御器。経過時間 dt は呼び出し側が渡す。
// 出力は ±out_limit にクランプし、飽和時は積分器を anti-windup する。
class Pid {
public:
    Pid(float kp, float ki, float kd, float out_limit)
        : kp_(kp), ki_(ki), kd_(kd), out_limit_(out_limit) {}

    void setGains(float kp, float ki, float kd) {
        kp_ = kp;
        ki_ = ki;
        kd_ = kd;
    }

    void reset();

    // 目標値と計測値から出力を計算し、内部状態を更新する。
    // dt_s が 0 なら積分・微分を進めず P 項のみを返す。
    float update(float target, float measured, float dt_s);

private:
    float kp_;
    float ki_;
    float kd_;
    float out_limit_;

    float integral_ = 0.0f;
    float prev_error_ = 0.0f;
};

}  // namespace domain
