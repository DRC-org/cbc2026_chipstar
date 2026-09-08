#include "doctest.h"
#include "domain/deadline.hpp"

TEST_CASE("受信割込みが監視時刻を追い越しても通信断にしない") {
 CHECK_FALSE(domain::deadlineExpired(1000, 1001, 250));
 CHECK_FALSE(domain::deadlineExpired(0xFFFFFFFFU, 0, 250));
}
TEST_CASE("実際の通信途絶だけ期限切れとする") {
 CHECK_FALSE(domain::deadlineExpired(1250, 1000, 250));
 CHECK(domain::deadlineExpired(1251, 1000, 250));
 CHECK_FALSE(domain::deadlineExpired(100, 0xFFFFFFF0U, 250));
 CHECK(domain::deadlineExpired(300, 0xFFFFFFF0U, 250));
}
