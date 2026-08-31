#pragma once

#include <cstddef>

namespace domain {

// バイト列から改行区切りの行を組み立てる。
// 容量を超えた行は捨てる。途中で切れた行を指令として解釈させないため。
class LineReader {
public:
    static constexpr std::size_t CAPACITY = 96;

    // 1 バイト投入する。行が完成したら true を返し、line() / length() で取り出せる。
    // 完成した行は次に push するまで有効。
    bool push(char ch);

    const char* line() const { return buffer_; }
    std::size_t length() const { return completed_length_; }

private:
    char buffer_[CAPACITY] = {};
    std::size_t length_ = 0;
    std::size_t completed_length_ = 0;
    bool overflow_ = false;
};

}  // namespace domain
