#include "doctest.h"

#include "domain/command_queue.hpp"

using domain::Command;
using domain::CommandKind;
using domain::CommandQueue;

namespace {
Command of(CommandKind kind) {
    Command command;
    command.kind = kind;
    return command;
}
}  // namespace

TEST_CASE("空のキューからは取り出せない") {
    CommandQueue queue;
    Command out;

    CHECK(queue.empty());
    CHECK_FALSE(queue.pop(out));
}

TEST_CASE("積んだ順に取り出せる") {
    CommandQueue queue;
    CHECK(queue.push(of(CommandKind::Run)));
    CHECK(queue.push(of(CommandKind::Home)));
    CHECK(queue.push(of(CommandKind::Stop)));

    Command out;
    REQUIRE(queue.pop(out));
    CHECK(out.kind == CommandKind::Run);
    REQUIRE(queue.pop(out));
    CHECK(out.kind == CommandKind::Home);
    REQUIRE(queue.pop(out));
    CHECK(out.kind == CommandKind::Stop);
    CHECK(queue.empty());
}

TEST_CASE("引数も一緒に運ばれる") {
    CommandQueue queue;
    Command enable = of(CommandKind::Enable);
    enable.mask = domain::slot_bit::SLOT1;
    enable.value = true;
    queue.push(enable);

    Command out;
    REQUIRE(queue.pop(out));
    CHECK(out.mask == domain::slot_bit::SLOT1);
    CHECK(out.value);
}

TEST_CASE("満杯になったら押し出さずに捨てる") {
    // 古い指令を上書きすると、先に積まれた STOP が消える。
    CommandQueue queue;
    for (std::size_t i = 0; i < CommandQueue::CAPACITY - 1; ++i) {
        CHECK(queue.push(of(CommandKind::Stop)));
    }
    CHECK_FALSE(queue.push(of(CommandKind::Run)));

    Command out;
    REQUIRE(queue.pop(out));
    CHECK(out.kind == CommandKind::Stop);
}

TEST_CASE("取り出した分だけ空きが戻る") {
    CommandQueue queue;
    for (std::size_t i = 0; i < CommandQueue::CAPACITY - 1; ++i) {
        queue.push(of(CommandKind::Stop));
    }

    Command out;
    queue.pop(out);
    CHECK(queue.push(of(CommandKind::Run)));
}

TEST_CASE("容量を超えて何度も往復できる") {
    // リングの巻き戻りで壊れないこと。
    CommandQueue queue;
    Command out;
    for (int i = 0; i < 100; ++i) {
        REQUIRE(queue.push(of(CommandKind::Home)));
        REQUIRE(queue.pop(out));
        CHECK(out.kind == CommandKind::Home);
    }
    CHECK(queue.empty());
}
