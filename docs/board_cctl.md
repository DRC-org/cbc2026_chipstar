# cctl 基板リファレンス

中央制御基板 `DRC-CCTL2026`（STM32G474, LQFP48 系ピン配置）のペリフェラル・ピン割当と、
実装時に踏みやすいハード固有の注意点をまとめる。値は `Core/Src/main.c`・
`Core/Inc/main.h`・`Core/Src/stm32g4xx_hal_msp.c` から確認済み。

FDCAN のビットレート設計・CAN ID 割当は [cctl_can_bus.md](cctl_can_bus.md) を参照。

## クロック

- HSI 16MHz → PLL（`PLLM=1, PLLN=20, PLLR=2`）→ **SYSCLK = 160MHz**
- AHB / APB1 / APB2 いずれも分周なし → **PCLK1 = PCLK2 = 160MHz**

## ペリフェラル

| MCU ペリフェラル | ハンドル | ピン | AF | 設定 | 用途 |
|---|---|---|---|---|---|
| FDCAN1 | `hfdcan1` | PB8=RX, PB9=TX | AF9 | 1Mbps, Classic | モータ用バス |
| FDCAN2 | `hfdcan2` | PB5=RX, PB6=TX | AF9 | 1Mbps, Classic | 周辺基板用 |
| FDCAN3 | `hfdcan3` | PB3=RX, PB4=TX | AF11 | ⚠ CubeMX 既定のまま | 予備 |
| I2C1 | `hi2c1` | PA15=SCL, PB7=SDA | AF4 | `Timing=0x30D29DE4`（≒100kHz） | LCD 専用 |
| I2C3 | `hi2c3` | PC8=SCL, PC9=SDA | AF8 | 同上 | 基板の **I2C1 コネクタ** |
| I2C4 | `hi2c4` | PC6=SCL, PC7=SDA | AF8 | 同上 | 基板の **I2C2 コネクタ** |
| USART3 | `huart3` | PC10=TX, PC11=RX | AF7 | 115200, 8-N-1 | USB シリアル |
| TIM15 CH2 | `htim15` | PB15 | AF1 | `PSC=159` → 1MHz カウント | ブザー（PWM） |
| TIM2 | `htim2` | — | — | `PSC=159, ARR=999` → **1kHz 割込み** | アプリの周期処理 |
| USB FS (CDC) | — | PA11=DM, PA12=DP | — | 仮想COMポート | ホストPC接続 |

ブザーは TIM15 のカウントが 1MHz なので、`ARR = 1000000 / 周波数[Hz] - 1`、
デューティ 50% は `CCR = (ARR + 1) / 2` で鳴らす。

## GPIO

| 信号 | ピン | 設定 | 論理 |
|---|---|---|---|
| LED1〜3 | PC1, PC2, PC3 | 出力 PP / 初期 LOW | HIGH で点灯 |
| SW1〜3 | PA10, PA9, PA8 | 入力 プルアップ | 閉で LOW |
| DIP1〜4 | PC14, PC15, PF0, PF1 | 入力 プルアップ | ON で LOW |
| I2C1_INT, I2C2_INT | PB0, PB1 | 入力 NOPULL | コネクタからの割込み線（未使用） |
| NRST | PG10 | 入力 NOPULL | — |

SW1〜3 は基板上のスイッチではなく、外部接点用の 2 ピンコネクタ J5 / J6 / J7
（pin 1=GND, pin 2=信号）である。10ms のデバウンス後の値を `STATE` の `sw=` に載せて
50ms ごとに送る。リミットとしての割当と原点出しは host 側で、
[host_machine_profile.md](host_machine_profile.md) を参照。

## LCD

秋月 AQM1602（ST7032 系）を I2C1 に接続。スレーブアドレスは **0x3E**（7bit）。
ドライバは `src/lcd_aqm1602.{h,cpp}`。16文字×2行。

## 注意点

### 基板のコネクタ名と MCU の I2C 番号が一致しない

基板シルクの `I2C1` / `I2C2` は、MCU 上では I2C3 / I2C4 に配線されている。
MCU の I2C1 は LCD 専用で、外部コネクタには出ていない。

| 基板上の名前 | MCU ペリフェラル | ハンドル |
|---|---|---|
| I2C1 コネクタ | I2C3 | `hi2c3` |
| I2C2 コネクタ | I2C4 | `hi2c4` |
| LCD | I2C1 | `hi2c1` |

`main.h` のピンマクロも基板側の名前で生成されているため、`I2C1_SCL_Pin`（=PC8）は
MCU の I2C1 ではなく I2C3 のピンを指す。ハンドルとピンマクロの対応を取り違えやすい。

### `ISC2_SDA` は綴り誤り

PC7 のラベルが `.ioc` の時点で `ISC2_SDA` になっており、`main.h` にも
`ISC2_SDA_Pin` として生成される。意味は `I2C2_SDA`。

### FDCAN3 は未整備

FDCAN2は1Mbpsに設定し、USB CDCから標準IDのCANフレームを送受信できる。FDCAN3は
CubeMX既定のタイミングのままで、アプリから開始していない。詳細は
[cctl_can_bus.md](cctl_can_bus.md)。

### USB CDC の受信は割込みコンテキスト

`USB_Device/App/usbd_cdc_if.c` の受信コールバックから `cctl_usbcdc_receive()` が
直接呼ばれる。ここでは受信バイトをバッファに積むだけにして、解釈や制御は `loop()` 側で行う。

## アプリ層の約束

`Core/Src/main.c` の USER CODE から `setup()` と `loop()` を呼ぶだけにし、実装は
`src/app.cpp` に置く。`src/app.h` が宣言する関数は次の3つで、いずれも定義が必須。

| 関数 | 呼び出し元 |
|---|---|
| `setup()` | `main()` の初期化直後に1回 |
| `loop()` | `main()` の無限ループから毎周 |
| `cctl_usbcdc_receive()` | USB CDC 受信コールバック |

ペリフェラル単体の動作確認コードは [samples/cctl/](../samples/cctl) にある。
