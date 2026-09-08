# serial_svmd 基板リファレンス

シリアルサーボ駆動基板 `DRC-SerialSVMD2026`（STM32F303K8T6, LQFP32）のペリフェラル・
ピン割当と、実装時に踏みやすいハード固有の注意点をまとめる。値は `Core/Src/main.c`・
`Core/Inc/main.h`・`Core/Src/stm32f3xx_hal_msp.c`・`src/app.cpp` とKiCad回路図から確認済み。

## クロック

- **HSI 8MHz を PLL なしで直結** → SYSCLK = 8MHz
- AHB / APB1 / APB2 いずれも分周なし → PCLK1 = PCLK2 = 8MHz、`FLASH_LATENCY_0`
- USART1 のクロック源は PCLK1

外部発振子は載っていない。CAN のビットレート精度もこの内蔵 RC 発振器に依存する。

## ペリフェラル

| MCU ペリフェラル | ハンドル | ピン | AF | 設定 | 用途 |
|---|---|---|---|---|---|
| USART1 | `huart1` | PA9=TX, PA10=RX | AF7 | 1Mbps, 8-N-1、8倍サンプリング | サーボ（絶縁・半二重） |
| USART2 | `huart2` | PB3=TX, PA15=RX | AF7 | 115200, 8-N-1 | USB シリアル |
| CAN | `hcan` | PA11=RX, PA12=TX | AF9 | 1Mbps, AutoBusOff 有効 | 上位との通信 |
| TIM3 | `htim3` | — | — | `PSC=0, ARR=65535`（未使用） | — |

CAN のタイミングは `Prescaler=1, BS1=5TQ, BS2=2TQ, SJW=2TQ`。
8MHz ÷ (1+5+2) TQ = **1Mbps**、サンプルポイント 75%。

## GPIO

| 信号 | ピン | 設定 | 論理 |
|---|---|---|---|
| LED1〜4 | PA3, PA2, PA1, PA0 | 出力 PP / NOPULL / 初期 LOW | HIGH で点灯 |
| LED5, LED6 | PF1, PF0 | 同上 | HIGH で点灯 |
| SW1〜6 | PB1, PB0, PA7, PA6, PA5, PA4 | 入力 プルアップ | 閉で LOW |
| DIP1〜4 | PB4, PB5, PB6, PB7 | 入力 プルアップ | ON で LOW |

SW1〜6 は基板上のスイッチではなく、外部接点用の 2 ピンコネクタ J4〜J9（pin 1=GND,
pin 2=信号）である。DIP1〜4 は基板上の 4 回路 DIP で、共通端子が GND に落ちている。

LED の番号とポート順が逆向き（LED1=PA3 … LED4=PA0、LED5=PF1, LED6=PF0）なので、
ビットマスクを機械的に組み立てると番号がずれる。

## サーボ通信回路

MCU の USART1 は、絶縁アンプ（ADuM121N）と RS-485 トランシーバ（MAX485E）、および
トランジスタ Q1 による送受信方向の自動切替回路を経由してサーボへ繋がる。
**MCU 側に方向制御 GPIO はない**ため、ドライバは送信後そのまま受信すればよい。

MAX485EのA端子がSIG、B端子が抵抗分圧の基準電圧へ繋がる構成で、サーボ側は
TTL単線であり、RS-485のA/B配線ではない。Q1がTXに応じてDEと受信許可を切り替える。

USART1 RXはDMA1 Channel5の循環バッファ256byteで受ける。送信前に読取り位置を
最新へ進め、受信時はID・長さ・チェックサムを検証する。古いACKや別IDの応答は読み飛ばす。
受信タイムアウトまたはUART異常の後は、次の命令を送る前にDMA受信を再初期化する。
再初期化に失敗した場合は送信せずHAL異常を返し、次の命令でも再初期化を試みる。
位置・レジスタ・一括監視のREADは通信異常時に一度だけ再試行する。動作命令は再送しない。
CANは自動再送を有効化し、調停負けや一時的な送信エラーによる確認応答の欠落を防ぐ。
1Mbpsでの立上り・方向切替時間は実基板の波形確認が必要。

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

### SW4 という名前が 2 つある

基板上の **SW4（SWCTL）** はサーボバスの相手を MCU（USART1）と USB_Servo コネクタ
（J16）で切り替えるスライドスイッチで、host から動かすときは MCU 側にする。
外部接点コネクタ **J7** の silk も「SW4」で、こちらは PA6 のプルアップ入力。別物である。

### STS3215 の通信速度

STS3215-C018の工場出荷時は1Mbps。`setup()`でUSART1を1Mbps、8倍サンプリング、
循環DMA受信へ初期化する。CubeMX生成コードの初期115200bps設定は、その後アプリ層で
上書きされる。8MHzで16倍サンプリングのまま1Mbpsを指定するとHALのBRR下限に抵触する。

`servo_baud`は基板UARTだけを変更し、サーボ本体の保存設定は変更しない。
ID・通信速度・動作モードの設定は [STS3215導入手順](sts3215_bringup.md) を参照。

## 電源とコネクタ

サーボ用電源 `+BATT` は J10（1=GNDPWR, 2=+BATT）から供給する。USB や ST-LINK の
電源だけではサーボを駆動できない。サーボコネクタ J11〜J14 は 1=GNDPWR, 2=+BATT, 3=SIG。
`+BATT`はサーボへ直結で、12Vへ降圧されない。STS3215-C018には12Vを供給する。

コネクタの一覧と基板間の接続は [wiring.md](wiring.md) にまとめている。

## アプリ層の約束

`Core/Src/main.c` の USER CODE から `setup()` と `loop()` を呼ぶだけにし、実装は
`src/app.cpp` に置く。宣言は `src/app.h`。

上位プロトコルは機体固有IDを固定せず、`HELLO`、`SAFE/RUN/STOP`、
`SERVO ENABLE/TARGET/READ`を受理する。USART2のASCIIと、cctl FDCAN2からの
CAN指令（標準ID `0x320`）の両方を同じ状態機械へ入れる。機体ではCANを使い、
USART2は基板単体で触るための口として残す。プロトコルと範囲は
[device_protocol.md](device_protocol.md)を参照。

ペリフェラル単体の動作確認コードは [samples/serial_svmd/](../samples/serial_svmd) にある。
