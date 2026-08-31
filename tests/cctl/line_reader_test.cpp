#include "doctest.h"

#include "domain/line_reader.hpp"

#include <string>
#include <vector>

using domain::LineReader;

namespace {
// 文字列を 1 バイトずつ流し込み、完成した行を集める。
std::vector<std::string> feed(LineReader& reader, const std::string& input) {
    std::vector<std::string> lines;
    for (const char ch : input) {
        if (reader.push(ch)) {
            lines.emplace_back(reader.line(), reader.length());
        }
    }
    return lines;
}
}  // namespace

TEST_CASE("改行で 1 行が完成する") {
    LineReader reader;
    const auto lines = feed(reader, "abc\n");

    REQUIRE(lines.size() == 1);
    CHECK(lines[0] == "abc");
}

TEST_CASE("CRLF の CR は落とす") {
    LineReader reader;
    const auto lines = feed(reader, "abc\r\n");

    REQUIRE(lines.size() == 1);
    CHECK(lines[0] == "abc");
}

TEST_CASE("空行も 1 行として完成する") {
    LineReader reader;
    const auto lines = feed(reader, "\n");

    REQUIRE(lines.size() == 1);
    CHECK(lines[0].empty());
}

TEST_CASE("複数行を続けて取り出せる") {
    LineReader reader;
    const auto lines = feed(reader, "one\ntwo\nthree\n");

    REQUIRE(lines.size() == 3);
    CHECK(lines[0] == "one");
    CHECK(lines[1] == "two");
    CHECK(lines[2] == "three");
}

TEST_CASE("改行が来るまでは完成しない") {
    LineReader reader;
    const auto lines = feed(reader, "no newline yet");

    CHECK(lines.empty());
}

TEST_CASE("容量ちょうどの行は通る") {
    LineReader reader;
    const std::string body(LineReader::CAPACITY, 'x');
    const auto lines = feed(reader, body + "\n");

    REQUIRE(lines.size() == 1);
    CHECK(lines[0].size() == LineReader::CAPACITY);
}

TEST_CASE("容量を超えた行は捨てる") {
    // 途中で切れた行を渡すと、誤った指令として解釈されうる。
    LineReader reader;
    const std::string body(LineReader::CAPACITY + 1, 'x');
    const auto lines = feed(reader, body + "\n");

    CHECK(lines.empty());
}

TEST_CASE("溢れた行の次から復帰する") {
    LineReader reader;
    const std::string body(LineReader::CAPACITY + 10, 'x');
    const auto lines = feed(reader, body + "\nrecovered\n");

    REQUIRE(lines.size() == 1);
    CHECK(lines[0] == "recovered");
}
