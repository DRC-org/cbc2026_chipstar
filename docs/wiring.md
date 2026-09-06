# 配線ガイド

現在の想定配線。回路図（`DRC-CCTL2026` / `DRC-SerialSVMD2026` / `DRC-DCMD2026.2ch-2E` /
`DRC-SVMD2025_Ver2.1`）から起こしたコネクタ表と、間違えやすい箇所をまとめる。

host の GUI からは「6 配線」タブで同じ内容を参照できる。

## 全体構成

PC から USB で繋ぐのは**コントローラと cctl だけ**。機体側のデバイスは cctl から
CAN で分配する。

```
PC ──USB──▶ DualSense（コントローラ）
   └─USB──▶ cctl  J12 (USB-C)          指令とテレメトリ 115200 baud
                   ├─ FDCAN1  J2 ──▶ EL05 / M3508+C620 / DM-S3519     1 Mbps
                   └─ FDCAN2  J3 ──▶ svmd ──▶ DCMD ──▶ serial_svmd    1 Mbps
                                       (各基板のCANコネクタ2個で数珠つなぎ)
                                                          └─ USART1 ──▶ STS3215 J11〜J14
```

serial_svmd の USART2（J15 の USB-C）は基板単体で触るための口として残っている。
機体としては使わないので、PC への USB は cctl の 1 本だけになる。

### 現在の機体プロファイル

`host/config/rtheta.toml` は cctl の 3 軸だけを構成しており、svmd・DCMD・serial_svmd は
どれも入っていない。周辺基板は動作テストで単体確認できる状態で、機体としてはまだ
組み込まれていない。

## 間違えやすい箇所

### 接点コネクタのピン順が基板で逆

| 基板 | pin 1 | pin 2 |
|---|---|---|
| cctl J5 / J6 / J7 | GND | 信号 |
| serial_svmd J4〜J9 | GND | 信号 |
| **DCMD J12 / J4 / J3** | **信号** | **GND** |

DCMD だけ逆順である。同じケーブルを挿し替えると短絡はしないが、接点が反応しない。

### USB-C が 1 枚に 2 個ある

| 基板 | host を繋ぐ側 | もう一方 |
|---|---|---|
| cctl | **J12**（USB CDC、MCU 直結） | J10（USB-UART、USART3 側） |
| serial_svmd | **J15**（USB-UART → USART2） | J16（USB_Servo、サーボバスへ直結） |

serial_svmd の J16 は MCU を経由せずサーボバスへ繋がる経路で、SW4 で切り替える。
Feetech の設定ツールでサーボを直接触るための口であり、host からの制御には使わない。

### serial_svmd の SW4 と SW3

- **SW4（SWCTL）**: サーボバスの相手を MCU（USART1）と J16（USB_Servo）で切り替える
  スライドスイッチ。host から動かすときは MCU 側にする。
  外部接点コネクタ J7 にも silk で「SW4」と振られており、別物なので注意する。
- **SW3**: CAN の終端抵抗 120Ω（R7）をバスへ入れるスイッチ。バスの端に置く基板だけ
  ON にする。serial_svmd を数珠つなぎの終端にするなら ON。

### DCMD のエンコーダコネクタは silk が両方 ENC0

J5 と J6 のどちらも silk が `ENC0` になっている。**ENC1（現 FW が使う方）は J6**。
`ENC_X1` が出ている方が J6 である。

### DM の Duty ゼロはブレーキ

DCMD の PWM はアクティブ Low で、Duty ゼロは両ローサイド ON のブレーキになる。
コーストでも電源遮断でもない。詳細は [board_dcmd.md](board_dcmd.md)。

## cctl（DRC-CCTL2026 / STM32G474）

| コネクタ | 用途 | ピン |
|---|---|---|
| J12 | **host との USB CDC** | USB-C |
| J10 | USB-UART（USART3、PC10/PC11） | USB-C |
| J2 | **FDCAN1** — モータ用 1 Mbps | 1=GND, 2=CAN_L, 3=CAN_H, 4=+5V |
| J3 | **FDCAN2** — 周辺基板用 1 Mbps | 1=GND, 2=CAN_L, 3=CAN_H, 4=+5V |
| J4 | FDCAN3（未整備・未使用） | 1=GND, 2=CAN_L, 3=CAN_H, 4=+5V |
| J5 | 接点 SW1（PA10） | 1=GND, 2=信号 |
| J6 | 接点 SW2（PA9） | 1=GND, 2=信号 |
| J7 | 接点 SW3（PA8） | 1=GND, 2=信号 |
| J8 | 基板 silk「I2C1」= MCU の **I2C3** | 1=GND, 2=+電源, 3=INT, 4=SCL, 5=SDA |
| J9 | 基板 silk「I2C2」= MCU の **I2C4** | 1=GND, 2=+電源, 3=INT, 4=SCL, 5=SDA |
| J1 | ST-LINK | 1=GND, 2=+電源, 3=+3V3, 4=NRST, 5=SWCLK, 6=SWDIO |

LCD（AQM1602 / 0x3E）は MCU の I2C1 に直結で、外部コネクタには出ていない。
基板 silk の I2C 番号と MCU の番号が一致しないので [board_cctl.md](board_cctl.md) を参照。

FDCAN1 に繋ぐモータの ID 割当は [cctl_can_bus.md](cctl_can_bus.md)。

| slot | デバイス | バス |
|---|---|---|
| 0 | RobStride EL05 | FDCAN1（拡張 ID） |
| 1 | M3508 + C620 | FDCAN1（標準 0x200 / 0x201） |
| 2 | DM-S3519 | FDCAN1（標準 0x109 / 0x00A） |

## serial_svmd（DRC-SerialSVMD2026 / STM32F303K8T6）

| コネクタ | 用途 | ピン |
|---|---|---|
| J1 / J2 | **CAN — 機体での接続経路（cctl FDCAN2）** | 1=GND, 2=CAN_L, 3=CAN_H, 4=+5V |
| J15 | USART2 の USB シリアル（115200 baud、基板単体で触る用） | USB-C |
| J16 | USB_Servo（サーボバス直結、SW4 で切替） | USB-C |
| J10 | サーボ電源 +BATT 入力 | 1=GNDPWR, 2=+BATT |
| J11〜J14 | STS3215 サーボ 1〜4 | 1=GNDPWR, 2=+BATT, 3=SIG |
| J4〜J9 | 接点 SW1〜SW6（PB1, PB0, PA7, PA6, PA5, PA4） | 1=GND, 2=信号 |
| J3 | ST-LINK | 1=GND, 2=+電源, 3=+3V3, 4=NRST, 5=SWCLK, 6=SWDIO |

サーボ電源は J10 から供給する。USB や ST-LINK の電源だけでサーボは駆動しない。
STS3215 は 115200 baud に設定しておく（工場出荷時に 1 Mbps の個体がある）。

## DCMD（DRC-DCMD2026.2ch-2E / STM32F303K8T6）

| コネクタ | 用途 | ピン |
|---|---|---|
| J7 / J8 | CAN（数珠つなぎ用に 2 個） | 1=GND, 2=CAN_L, 3=CAN_H, 4=+5V |
| J10 | PWM0 出力（DC モータ） | 1, 2 = モータ |
| J11 | PWM1 出力（現 FW では非駆動） | 1, 2 = モータ |
| J9 | モータ電源 Vm | 1=GNDPWR, 2=Vm |
| J6 | **ENC1**（現 FW が使う。silk は ENC0） | 1=GND, 2=X 相, 3, 4=+5V, 5 |
| J5 | ENC0（現 FW では未使用） | 1=GND, 2=X 相, 3, 4=+5V, 5 |
| J12 | 接点 SW_A（PB3） | **1=信号, 2=GND** |
| J4 | 接点 SW_B（PB4） | **1=信号, 2=GND** |
| J3 | 接点 SW_C（PB5） | **1=信号, 2=GND** |
| J1 | ST-LINK | 1=GND, 2=+電源, 3=+3.3V, 4=RESET, 5=SWCLK, 6=SWDIO |
| J2 | USB-C | — |

基板上の SW6（2 回路 DIP）が DIP1（PA7）/ DIP2（PA5）。SW5 は BOOT0、SW1 はリセット。

## svmd（DRC-SVMD2025_Ver2.1 / Arduino UNO R4 Minima）

| コネクタ | 用途 | ピン |
|---|---|---|
| J7 / J8 | CAN（数珠つなぎ用に 2 個） | 1=GND, 2=CAN_L, 3=CAN_H, 4=+5V |
| J9 | サーボ電源 +5V 入力 | 1=GNDPWR, 2=+5VP |
| J2 | SV0 | 1=GNDPWR, 2=+5VP, 3=SIG |
| J4 | SV1 | 1=GNDPWR, 2=+5VP, 3=SIG |
| J3 | SV2 | 1=GNDPWR, 2=+5VP, 3=SIG |
| J5 | SV3 | 1=GNDPWR, 2=+5VP, 3=SIG |
| J1 | USB（書き込み） | USB-C |

FW のチャネル 0〜3 は R4 の D3 / D6 / D10 / D9。**チャネル番号と SV の silk 番号の
対応は実機で確認する**（動作テストで 1 チャネルずつ動かすのが早い）。
基板上の SW2 が ID 用 DIP（A0〜A3）で、現 FW では読んでいない。

## 配線後の確認

組み上げた状態のまま確認するには、host の動作テストで対象に「ネットワーク一括」を
選ぶ。cctl の USB 1 本で cctl・svmd・DCMD を繋ぎ替えずに扱え、応答しなかった基板は
一覧に出る。手順は [firmware_tests.md](firmware_tests.md) を参照。

## 電源とグラウンド

- モータ動力とロジックは別系統にする。GND は共通に落とす。
- CAN コネクタの 4 番ピンは +5V で、バスを通じて配られる。供給元を 1 箇所に決める。
- CAN は 1 Mbps。終端 120Ω はバスの両端だけに入れる。
- 大会規定の非常停止は、動力電源をマイコンを介さず直接切る配線で成立させる。
  接点入力（SW）はこれを代替しない。
