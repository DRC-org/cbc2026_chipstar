#pragma once
#include <cstdint>

namespace domain {
// 受信割込みがnowの取得後に時刻を更新した場合は、期限切れにしない。
// 通常の32bit tick周回は符号なし差分で処理する。
inline bool deadlineExpired(uint32_t now, uint32_t last_contact, uint32_t timeout) {
    const uint32_t elapsed = now - last_contact;
    return elapsed < 0x80000000U && elapsed > timeout;
}
}  // namespace domain
