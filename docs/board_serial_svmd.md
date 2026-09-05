# serial_svmd 基板リファレンス

シリアルサーボ駆動基板 `DRC-SerialSVMD2026`（STM32F303K8T6, LQFP32）のペリフェラル・
ピン割当と、実装時に踏みやすいハード固有の注意点をまとめる。値は `Core/Src/main.c`・
`Core/Inc/main.h`・`Core/Src/stm32f3xx_hal_msp.c` から確認済み。

## クロック

- **HSI 8MHz を PLL なしで直結** → SYSCLK = 8MHz
- AHB / APB1 / APB2 いずれも分周なし → PCLK1 = PCLK2 = 8MHz、`FLASH_LATENCY_0`
- USART1 のクロック源は PCLK1

外部発振子は載っていない。CAN のビットレート精度もこの内蔵 RC 発振器に依存する。

## ペリフェラル

| MCU ペリフェラル | ハンドル | ピン | AF | 設定 | 用途 |
|---|---|---|---|---|---|
| USART1 | `huart1` | PA9=TX, PA10=RX | AF7 | 115200, 8-N-1 | サーボ（絶縁・半二重） |
| USART2 | `huart2` | PB3=TX, PA15=RX | AF7 | 38400, 8-N-1 | USB シリアル |
| CAN | `hcan` | PA11=RX, PA12=TX | AF9 | 1Mbps, AutoBusOff 有効 | 上位との通信 |
| TIM3 | `htim3` | — | — | `PSC=0, ARR=65535`（未使用） | — |

CAN のタイミングは `Prescaler=1, BS1=5TQ, BS2=2TQ, SJW=2TQ`。
8MHz ÷ (1+5+2) TQ = **1Mbps**、サンプルポイント 75%。

## GPIO

| 信号 | ピン | 設定 | 論理 |
|---|---|---|---|
| LED1〜4 | PA3, PA2, PA1, PA0 | 出力 PP / NOPULL / 初期 LOW | HIGH で点灯 |
| LED5, LED6 | PF1, PF0 | 同上 | HIGH で点灯 |
| SW1〜6 | PB1, PB0, PA7, PA6, PA5, PA4 | 入力 プルアップ | 押下で LOW |
| DIP1〜4 | PB4, PB5, PB6, PB7 | 入力 **NOPULL** | 外部プルに依存 |

LED の番号とポート順が逆向き（LED1=PA3 … LED4=PA0、LED5=PF1, LED6=PF0）なので、
ビットマスクを機械的に組み立てると番号がずれる。

## サーボ通信回路

MCU の USART1 は、絶縁アンプ（ADuM121N）と RS-485 トランシーバ（MAX485E）、および
トランジスタ Q1 による送受信方向の自動切替回路を経由してサーボへ繋がる。
**MCU 側に方向制御 GPIO はない**ため、ドライバは送信後そのまま受信すればよい。

半二重なので、送信直後に自分の送出データを受信バッファに拾うことがある。
`src/sts3215.cpp` では送信前に `UART_RXDATA_FLUSH_REQUEST` で受信バッファを捨てている。

## 注意点

### PF0 / PF1 は発振子ピンとの兼用

PF0=OSC_IN, PF1=OSC_OUT のピンを LED6 / LED5 に割り当てている。HSI 動作で外部発振子を
使わないため成立している構成で、外部クロックへ変更するとこの2つの LED は使えなくなる。

### CAN 1Mbps は同期余裕が小さい

8MHz からの 1Mbps では 1 ビットが 8TQ しか取れず、SJW は BS2（2TQ）に制約されて
最大 2TQ になる。内蔵 HSI の周波数誤差に対する再同期の余裕は、より低速で TQ 数を
稼ぐ構成に比べて小さい。通信が不安定な場合は BS1/BS2 の配分見直しか低速化を検討する。

### 回路図の CAN_RX / CAN_TX 表記は逆

STM32F303K8T6 の CAN は PA11=RX, PA12=TX に固定されており、コードもそれに従う。
回路図上の信号名とは一致しないので、配線を追うときに注意する。

### DIP スイッチにプルがない

DIP1〜4 は `GPIO_NOPULL` で生成されている。SW1〜6 がプルアップなのと異なり、
基板側に外部プルがなければ読み取り値は不定になる。使う前に配線を確認する。

### STS3215 の通信速度

STS3215 は工場出荷時 1Mbps の個体がある。USART1 は 115200bps 固定なので、
サーボ側を事前に 115200bps へ変更しておく。STS プロトコルの Baud Rate レジスタは
アドレス6、115200bps の設定値は 4。1Mbps のサーボへ 115200bps のプログラムから
設定を変更することはできないため、Feetech の設定ツール等で先に変更して電源を入れ直す。

## 回路図で確認したい項目

以下は基板資料に基づく記述で、ソースコードからは裏付けが取れない。実機を触る前に
回路図で確認する。

- **サーボ電源**: `+BATT` はサーボ用電源で、J10 から供給する。USB / ST-LINK の電源だけで
  サーボを駆動しない。サーボコネクタ J11〜J14 は 1=GNDPWR, 2=+BATT, 3=SIG。
- **SW4 の役割**: MCU の USART1 とサーボ駆動回路の接続を切り替えるスイッチとして
  説明されている一方、`.ioc` 上では PA6 のプルアップ入力にも割り当てられている。
  同一の部品を指しているのか別系統かを確認する。

## アプリ層の約束

`Core/Src/main.c` の USER CODE から `setup()` と `loop()` を呼ぶだけにし、実装は
`src/app.cpp` に置く。宣言は `src/app.h`。

USART2の上位プロトコルは機体固有IDを固定せず、`HELLO`、`SAFE/RUN/STOP`、
`SERVO ENABLE/TARGET/READ`を受理する。プロトコルと範囲は
[device_protocol.md](device_protocol.md)を参照。

ペリフェラル単体の動作確認コードは [samples/serial_svmd/](../samples/serial_svmd) にある。
