# cctl CAN バス・クロック構成

`cctl`（STM32G474, `DRC-CCTL2026`）の FDCAN 周辺と、rθz モータ通信のためのバス割当・
CAN ID 設計をまとめる。値は `Core/Src/main.c`・`stm32g4xx_hal_msp.c` から確認済み。

## クロックとビットレート

SystemClock（`main.c` の `SystemClock_Config`）:

- PLL 入力 = HSI 16MHz、`PLLM=1, PLLN=20, PLLR=2` → **SYSCLK = 160MHz**
- `AHB=1, APB1=1, APB2=1` → **PCLK1 = 160MHz**
- FDCAN カーネルクロック源 = **PCLK1（160MHz）**（`FdcanClockSelection = RCC_FDCANCLKSOURCE_PCLK1`）

ビットレート = `fdcan_clk / (Prescaler × (1 + Seg1 + Seg2))`。

| ペリフェラル | Prescaler | Seg1 | Seg2 | 合計tq | ビットレート | 状態 |
|---|---|---|---|---|---|---|
| FDCAN1 | 16 | 7 | 2 | 10 | **1 Mbps** | 正常（モータ用） |
| FDCAN2 | 16 | 1 | 1 | 3 | 3.33 Mbps | ⚠ CubeMX 既定のまま・未整備 |
| FDCAN3 | 16 | 1 | 1 | 3 | 3.33 Mbps | ⚠ CubeMX 既定のまま・未整備 |

> **注意**: FDCAN2/3 は 1Mbps になっていない。周辺基板バスとして使う際は `.ioc` で
> `NominalTimeSeg1=7, NominalTimeSeg2=2`（1Mbps）等に設定し直すこと。全ペリフェラル
> Classic CAN（`FDCAN_FRAME_CLASSIC`）、`AutoRetransmission=DISABLE`。

## ピン割当（`stm32g4xx_hal_msp.c`）

| ペリフェラル | RX | TX | AF |
|---|---|---|---|
| FDCAN1 | PB8 | PB9 | AF9 |
| FDCAN2 | PB5 | PB6 | AF9 |
| FDCAN3 | PB3 | PB4 | AF11 |

## バス割当方針

3系統を用途で分離する。

| バス | 用途 | ビットレート |
|---|---|---|
| **FDCAN1** | **全モータ（DM / EL05 / M3508）を集約** | 1 Mbps |
| FDCAN2 | 周辺基板 | 未定（要設定） |
| FDCAN3 | 予備（未使用） | — |

モータは 3 種とも 1Mbps のため、唯一 1Mbps 設定済みの FDCAN1 に集約する。

## CAN ID 割当（同一バス上の衝突回避）

1本のバスに DM・EL05・M3508 を混載するため、**標準IDの衝突**に注意が必要。
EL05 は拡張ID(29bit)なので標準IDと空間が別で衝突しない。**DM と M3508/C620 が標準IDで競合**する。

- C620(θ) はコマンド `0x200`、フィードバック `0x200 + ESC_ID`（ID=1 → `0x201`）を占有。
- DM のコマンドは `0x100 + CAN_ID`。CAN_ID を 0〜8 にすると C620 の `0x200〜0x208` と衝突する
  （※ DM は `0x100+` 系なので実際に重なるのは稀だが、帯域と識別のため明確に分離する）。

採用した割当（`cctl/src/robot_config.hpp` の `can_id` 名前空間）:

| モータ | 種別 | コマンドID | フィードバックID | 備考 |
|---|---|---|---|---|
| M3508 / C620 (θ) | 標準 | `0x200` | `0x201` | ESC ID = 1 |
| DM-S3519 (z) | 標準 | `0x109`（=0x100+0x09） | `0x00A` | CAN_ID=0x09 / MST_ID=0x0A |
| EL05 (r) | 拡張 | 拡張ID | 拡張ID | motor=0x7F / host=0xFD |

受信振り分けは **`RxHeader.IdType`（標準/拡張）＋ Identifier** で判定する
（`RThetaZController::dispatchRx`）。

## 受信・送信の実装方針

- グローバルフィルタは**全ID受理→RX FIFO0**（`FDCAN_ACCEPT_IN_RX_FIFO0`, `FDCAN_REJECT_REMOTE`）。
- 受信は **FIFO0 のポーリング**（`HAL_FDCAN_GetRxFifoFillLevel` → `GetRxMessage`）。割込み未使用。
- 送信は `HAL_FDCAN_AddMessageToTxFifoQ`、全フレーム 8byte・Classic CAN・BRS off。

## バス負荷の目安

制御周期（`period` 名前空間）:

- M3508 電流ループ: 1kHz（`0x200` 送信＋FB受信）
- DM 位置速度指令: 100Hz 再送
- EL05 `LOC_REF`: 50Hz 更新

1Mbps で 8byte 標準フレーム ≈ 130µs。送受合わせて概ね数百µs/ms のため、上記配分なら余裕がある。
θ の応答を上げる場合は M3508 レートを優先し、DM/EL05 の再送レートを絞る。
