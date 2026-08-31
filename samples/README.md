# samples

各基板のペリフェラルを単体で動かして確認するためのコード。ファームウェア本体の
ビルド対象には含まれない。

ピン割当・ハンドル・ハード固有の制約は [docs/board_cctl.md](../docs/board_cctl.md) /
[docs/board_serial_svmd.md](../docs/board_serial_svmd.md) を参照。

## 使い方

対象プロジェクトの `src/app.cpp` を差し替えてビルドする。

```sh
cp samples/cctl/lcd/app.cpp cctl/src/app.cpp
cd cctl && cmake --build --preset Debug
```

確認後は `git checkout -- cctl/src/app.cpp` で元に戻す。

各サンプルは `src/app.h` が宣言する関数をすべて実装する。cctl は `setup()` と
`loop()` に加えて `cctl_usbcdc_receive()` の定義が必要で、欠けるとリンクに失敗する。

`src/` に置いたドライバ（cctl の `lcd_aqm1602`）はサンプルからも参照できる。

## cctl（STM32G474 / DRC-CCTL2026）

| サンプル | 動作 | 使用ペリフェラル |
|---|---|---|
| `buzzer` | 起動時に2音鳴らす | TIM15 CH2 (PB15) |
| `can` | 受信したフレームを同じIDで送り返す | FDCAN1 (PB8/PB9) |
| `dipsw` | DIP1〜4 を1バイトに詰めて `dip_value` に反映 | GPIO (PC14/PC15/PF0/PF1) |
| `i2c` | 0x08〜0x77 をスキャンし `detected_addresses` に格納 | I2C4 = 基板の I2C2 コネクタ |
| `lcd` | AQM1602 に文字列を表示 | I2C1 (LCD 専用) |
| `sw` | SW1〜3 の押下状態を変数に反映 | GPIO (PA8/PA9/PA10) |
| `usart` | 受信した1バイトをそのまま返す | USART3 (PC10/PC11) |
| `usbcdc` | 受信データをそのまま返す | USB CDC (PA11/PA12) |

`dipsw` / `sw` / `i2c` は結果を変数に置くだけなので、デバッガの Live Expressions か
Watch で確認する。

## serial_svmd（STM32F303K8T6 / DRC-SerialSVMD2026）

| サンプル | 動作 | 使用ペリフェラル |
|---|---|---|
| `led` | LED1〜6 を順に点灯させ、最後に全点灯 | GPIO (PA0〜PA3, PF0, PF1) |
| `sw` | SWn の押下中に LEDn を点灯 | GPIO |
| `can_rx` | ID 0x123 のフレームを受信し、受信状況を変数に反映 | CAN (PA11/PA12) |

`can_rx` は `g_can_status_code` ほかの変数を Live Expressions で監視する前提。
状態コードの意味はソース冒頭のコメントに記載。
