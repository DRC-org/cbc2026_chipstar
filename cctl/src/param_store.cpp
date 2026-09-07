#include "param_store.hpp"

#include "stm32g4xx_hal.h"

#include <cstring>

namespace param_store {
namespace {

constexpr uint32_t MAGIC = 0x43504152UL;  // 'CPAR'
constexpr uint32_t VERSION = 1;

struct Record {
    uint32_t magic;
    uint32_t version;
    uint32_t count;
    uint32_t hash;
    float values[domain::PARAM_COUNT];
};

// Flash はダブルワード単位でしか書けない。
constexpr uint32_t RECORD_WORDS = (sizeof(Record) + 7) / 8;

bool dual_bank() { return (FLASH->OPTR & FLASH_OPTR_DBANK) != 0; }

// 最終ページを1枚だけ使う。単バンク構成ではページが4KBになり位置が変わる。
uint32_t record_address() { return dual_bank() ? 0x0807F800UL : 0x0807F000UL; }
uint32_t record_bank() { return dual_bank() ? FLASH_BANK_2 : FLASH_BANK_1; }

uint32_t hash(const float* values, uint32_t count) {
    uint32_t result = 2166136261UL;
    const auto* bytes = reinterpret_cast<const uint8_t*>(values);
    for (uint32_t i = 0; i < count * sizeof(float); ++i) {
        result = (result ^ bytes[i]) * 16777619UL;
    }
    return result;
}

const Record& record() { return *reinterpret_cast<const Record*>(record_address()); }

bool valid(const Record& record) {
    return record.magic == MAGIC && record.version == VERSION &&
           record.count == domain::PARAM_COUNT &&
           record.hash == hash(record.values, record.count);
}

bool erasePage() {
    FLASH_EraseInitTypeDef erase = {};
    erase.TypeErase = FLASH_TYPEERASE_PAGES;
    erase.Banks = record_bank();
    erase.Page = 127;  // どちらの構成でも各バンク128ページ。
    erase.NbPages = 1;
    uint32_t error = 0;
    return HAL_FLASHEx_Erase(&erase, &error) == HAL_OK && error == 0xFFFFFFFFUL;
}

bool present_ = false;

}  // namespace

bool load(domain::Parameters& parameters) {
    const Record& saved = record();
    if (!valid(saved)) return false;
    if (!parameters.restore(saved.values, saved.count)) return false;
    present_ = true;
    return true;
}

bool save(const domain::Parameters& parameters) {
    Record fresh = {};
    fresh.magic = MAGIC;
    fresh.version = VERSION;
    fresh.count = domain::PARAM_COUNT;
    for (uint32_t id = 0; id < domain::PARAM_COUNT; ++id) {
        fresh.values[id] = parameters.get(static_cast<uint8_t>(id));
    }
    fresh.hash = hash(fresh.values, fresh.count);
    if (std::memcmp(&fresh, &record(), sizeof(Record)) == 0) return true;

    if (HAL_FLASH_Unlock() != HAL_OK) return false;
    bool ok = erasePage();
    uint64_t words[RECORD_WORDS] = {};
    std::memcpy(words, &fresh, sizeof(Record));
    for (uint32_t i = 0; ok && i < RECORD_WORDS; ++i) {
        ok = HAL_FLASH_Program(FLASH_TYPEPROGRAM_DOUBLEWORD,
                               record_address() + i * 8, words[i]) == HAL_OK;
    }
    HAL_FLASH_Lock();
    present_ = present_ || ok;
    return ok;
}

bool clear() {
    if (!valid(record())) return true;
    if (HAL_FLASH_Unlock() != HAL_OK) return false;
    const bool ok = erasePage();
    HAL_FLASH_Lock();
    if (ok) present_ = false;
    return ok;
}

bool present() { return present_; }

}  // namespace param_store
