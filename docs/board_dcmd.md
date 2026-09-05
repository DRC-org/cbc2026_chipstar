# DCMD 基板とFW

`dcmd/`はSTM32F303K8T6搭載の2ch DCモータ基板用FW。PWM0/PWM1テスト、ENC1手動
テストと`DRC-DCMD2026.2ch-2E`回路図に基づく。

## 配線と駆動

| 機能 | MCUピン | 用途 |
|---|---|---|
| PWM0 | PA2 / PA3 | TIM2 CH3 / CH4、J10 |
| PWM1 | PB0 / PB1 | TIM3 CH3 / CH4、J11 |
| CAN | PA11 / PA12 | RX / TX、MCP2562 |
| ENC0 A/B/X | PA0 / PA1 / PB6 | 現在のFWでは未使用 |
| ENC1 A/B/X | PA4 / PA6 / PB7 | 現在のFWでは未使用 |

HSI 8MHz、CAN 1Mbps（1+5+2 TQ）、PWM 20kHz（PSC=0、ARR=399）。
TLP2348で信号が反転するためPWMはアクティブLow。Dutyゼロは両出力Highとなり、
両ローサイドONのブレーキ状態になる。コーストや電源遮断ではない。
PWM開始前のGPIOもHighに設定し、起動時は100msのブートストラップ充電を行う。

正DutyはCH3をPWM、CH4をHighにする。負Dutyは逆。サンプルによれば正Duty時は
J10/J11のpin 2がpin 1に対して正となる。実機で極性を確認する。

## 汎用制御と停止

hostがチャネルと符号付きDutyを指定する。FWは±900 permille（±90%）を絶対上限とし、
10msごとに1 permilleずつ出力を変更する。方向反転時はゼロまで減速し、2秒間
ブレーキを維持してから逆方向へ立ち上げる。STOP/SAFE/通信断はランプを待たずゼロにする。

起動はSAFE。HELLOと対象チャネルのTARGETを受理してからRUNを許可する。
250msを超えてTARGET/HEARTBEATが途切れるとSTOPになり、再びHELLO、TARGET、RUNが
必要になる。HELLOだけでは通信期限を延長しない。

## CAN v1

cctl FDCAN2に接続する。指令は標準ID `0x310`（784）、状態は`0x311`（785）。
1バス上のDCMDは1台を想定する。複数台にはアドレス割当の拡張が必要。

指令は8 byte固定。

| byte | 内容 |
|---|---|
| 0 | version=1 |
| 1 | 0=HELLO、1=SAFE、2=RUN、3=STOP、4=TARGET、5=HEARTBEAT |
| 2 | TARGETはchannel 0/1、RUNは有効mask 1..3、それ以外は0 |
| 3 | 0 |
| 4..5 | TARGETの符号付き16bit Duty [permille]、big endian。それ以外は0 |
| 6..7 | 0 |

状態は`[1, result, mode, enabled, duty0_hi, duty0_lo, duty1_hi, duty1_lo]`。
resultは0=OK、1=拒否、2=timeout、modeは0=SAFE、1=RUN、2=STOP。
指令受信時と50ms周期で送る。cctlの`CAN_RX`通知はUSB混雑時に欠落し得るため、
通知一件だけを到達保証として扱わない。

## ビルドとhost

```sh
cd dcmd
bash generate.sh
cmake --preset Debug
cmake --build --preset Debug
```

生成にはCubeMX 6.15.0を使う。アプリは`src/`に置き、生成コードには初期化前の
ブレーキとsetup/loopの接続を追加する。

hostの`config/dcmd.toml`は2ch、最大Duty10%の立ち上げ用プロファイル。
hostディレクトリから`cargo run --locked -- --machine-profile config/dcmd.toml`で起動する。
既存プロファイルへ`[[dc_motors]]`を追加して混在させられる。
Dutyは入力軸の値から生成し、FWに機体固有の動作を保持しない。

## エンコーダの制約

PWM0とENC0はTIM2、PWM1とENC1はTIM3のカウンタを共有する。エンコーダモードで
カウンタを外部パルス駆動すると、同じタイマで20kHz PWMを生成できない。
現在のFWは2ch PWMを提供し、エンコーダ読取りは未実装。
同時使用にはソフトウェア計数の入力レート検証、または外部カウンタ等が必要になる。
サンプルの2048 PPR・4逓倍は機体側の設定値であり、汎用FWには固定しない。

## 実機確認

資料ではVm=12V、JP2=2-3、IR2302の/SD=High。ロジック電源とモータ電源は別系統。
最初に電流制限電源・モータ未接続で、起動時High、20kHz、Duty、方向切替を測定する。
通信断・STOP・CPU停止時の挙動は実機確認が必要。CPU停止時にはタイマがPWMを
保持し得るため、通電中のデバッガ停止を非常停止の代わりに使用しない。
