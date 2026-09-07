# STS3215-C018 設定・初回動作確認

対象は秋月販売コード130969の **STS3215-C018（12V版）**。
TTL半二重とは、1本の信号線を送信と受信で共有する通信方式。一般的なPWMサーボや
RS-485サーボとは接続方法が異なる。

## 使用する設定

| 項目 | 設定・意味 |
|---|---|
| 電源 | 定格12V。仕様範囲4〜14V。USB給電だけでは駆動できない |
| 通信 | TTL半二重、8N1、**1,000,000bps**を基本とする |
| ID | 出荷時1。1台ずつ、同じバス内で重複しない1〜253へ設定 |
| 動作モード | レジスタ33 = **0（位置制御）** |
| 位置 | 0〜4095、出力軸1回転を4096分割。2048は約180° |
| 書込み応答 | SerialSVMDは既定でSYNC_WRITE後にレジスタを読戻す |
| サーボ返信設定 | READに応答できる設定を維持する。返信を全面禁止しない |

内部減速比1:345を位置値へ再度掛けたり割ったりしない。0/4095の境界をまたぐ機構や
多回転を使う場合は別途設計が必要。現在のFWは位置モード0だけを扱う。
30kg・cmは瞬間的な拘束トルクで、連続使用の定格負荷は10kg・cm。12Vでの拘束電流は
1台2.7Aの仕様なので、電源・配線は複数台の同時負荷に合わせる。

## 電源と接続

1. 電源を切ってから、J10の1番へGND、2番へ12Vを接続する。
2. サーボはJ11〜J14へ接続する。1番GND、2番電源、3番SIG。
3. 初回のID設定ではサーボを**1台だけ**接続する。同じIDの複数台を繋いだまま変更しない。
4. 基板ロジック側にも給電する。J10の電源はサーボ側へ直結で、24Vから12Vへの降圧機能はない。

| 用途 | PC接続とSWCTL |
|---|---|
| ID・baud・モードの設定 | **J16 USB_Servo**、SWCTLをUSB側 |
| 通常のhost GUI操作 | PC→CCTL USB→FDCAN2→SerialSVMD CAN、SWCTLをMCU側 |
| FWのASCII単体テスト | J15 USB_Upstream、SWCTLをMCU側 |

J15とJ16は別用途。J16の直接設定にはSerialSVMDのFW指令を送らない。
SWCTLの左右は基板の見方で変わる。回路図・フットプリント上ではMCU経路が接点1–2と5–6、
USB経路が2–3と4–5に対応する。向きが不明なら電源OFFで導通を確認する。
CANは1Mbps。hostはSerialSVMDの基板アドレス0を使用するため、DIP1・2をOFFにして起動する。
CAN終端はバスの両端だけに入れる。詳細は[配線ガイド](wiring.md)。

## Linuxでの設定

リポジトリ直下で実行する。専用ツールは`serial_svmd/tools/sts3215_setup.py`。
pyserialを使うため、必要なら公式のvenv/pip手順で環境を作る。

```sh
python3 -m venv ~/.venvs/sts3215
~/.venvs/sts3215/bin/pip install pyserial
```

以下の`PORT`にはJ16に対応する`/dev/serial/by-id/...`を指定する。
シリアルモニタや他の設定ソフトは閉じておく。

```sh
PORT=/dev/serial/by-id/実際のUSB_Servoデバイス
~/.venvs/sts3215/bin/python serial_svmd/tools/sts3215_setup.py --port "$PORT" scan
~/.venvs/sts3215/bin/python serial_svmd/tools/sts3215_setup.py --port "$PORT" --id 1 inspect
```

既定は1Mbps、scanはID0〜20。必要なら`scan --last-id 253`で全IDを調べる。
応答がなければ電源・ポート・SWCTLを確認する。設定済みbaudが不明なら、対応する各baudを
`--baud`で指定して同じ読取りを行う。scan/inspectはサーボの保存設定や位置を変更しない。

ID1をID2へ変更する例：

```sh
~/.venvs/sts3215/bin/python serial_svmd/tools/sts3215_setup.py --port "$PORT" --id 1 --single-servo set-id 2
```

ツールはトルクをOFFにし、EPROM（電源OFFでも残る設定）のロックを解除、変更後のIDで
読戻し、再ロックする。**電源を入れ直してから**次を実行し、保存を確認する。

```sh
~/.venvs/sts3215/bin/python serial_svmd/tools/sts3215_setup.py --port "$PORT" --id 2 inspect
```

各個体にIDを表示してから次の1台へ交換する。EE回転=1、ボーナスのフタ=2・整列=3のように
割り当てられるが、実際のhostプロファイルもそのIDに揃える。

`inspect`のmodeが0以外なら、1台接続のまま次を実行する。

```sh
~/.venvs/sts3215/bin/python serial_svmd/tools/sts3215_setup.py --port "$PORT" --id 2 --single-servo position-mode
```

baud変更は通常不要。過去に115200へ変更した個体を1Mbpsへ戻す場合：

```sh
~/.venvs/sts3215/bin/python serial_svmd/tools/sts3215_setup.py --port "$PORT" --baud 115200 --id 2 --single-servo set-baud 1000000
```

共通オプションは`inspect`や`set-id`より前に置く。設定途中で失敗した場合はID/baudが
既に変わっている可能性があるため、新旧の値で再探索する。再通電後の確認までを設定完了とする。

### baudレジスタ

| baud | レジスタ6の値 |
|---|---|
| 1000000 | 0 |
| 500000 | 1 |
| 250000 | 2 |
| 128000 | 3 |
| 115200 | 4 |
| 76800 | 5 |
| 57600 | 6 |
| 38400 | 7 |

hostの`servo_baud`は**基板UART側だけ**を変更する。サーボのレジスタ6を書き換える機能ではない。

## Windowsの設定ソフトを使う場合

Feetech FDを使い、J16のCOMポートとBaudR=1000000を選択してOpen→Search。
1台だけ検出されたことを確認し、Programmingで対象を選択、`5 ID`へ新IDを入力してSaveする。
電源を入れ直してSearchし、新IDで検出されることを確認する。
ボタン名は[秋月の導入資料](datasheets/feetech_digital_servo_protocol_20220729.pdf)のFD版に基づく。
同資料の他型番向け電源例は流用せず、C018の12V仕様に従う。

## host GUIで初回動作確認

ID・baud設定後、電源を切ってSWCTLをMCU側に戻し、CCTLとCAN接続する。
SerialSVMDには本変更を含むFWの書込みが必要。
1台目（ID1）を確認するためのプロファイルを用意している。

```sh
cargo run --manifest-path host/Cargo.toml --bin host -- --machine-profile host/config/serial_svmd_bringup.toml
```

1. 接続・設定適用状態を確認する。
2. 個別テストでSTS3215のID1を選び、現在位置を読み取る。
3. 目標を現在位置に合わせ、速度100・加速度20程度で始める。初期値2048をそのまま送らない。
4. 機構に干渉しないことを確認し、現在位置から±20カウント程度の小さい位置変更を試す。
5. 実測位置の変化、停止・再開、タブ移動による個別テスト終了を確認する。

このプロファイルは通常のrθz操作を含まない。サーボは初期無効で、個別テストから操作する。
通常運転へ組み込む際は、確定したID・可動範囲・初期位置を機体プロファイルへ反映する。
Spaceはソフト緊停で、解除しても自動RUNしない。

## エラーの見方

GUIに`STS3215 ID ... 通信診断`が表示される。FWは異常時にSTOPへ移行し、トルクOFFを試みる。

| result | 調べる項目 |
|---|---|
| 2 HAL | USART初期化・DMA受信・UARTエラー |
| 3 timeout | 電源、ID、baud、J15/J16、SWCTL、信号線 |
| 4 形式 / 5 checksum | ID重複、波形、配線、返信データ |
| 6 サーボ異常 | flags、電圧、温度、過負荷・拘束 |
| 7 モード不一致 | 単体設定でレジスタ33を確認 |
| 8 読戻し不一致 | 書込みが適用されていない。設定・通信を再確認 |

トルクOFFの読戻しが取れない間はRUNを拒否する。通信断ではトルクOFF命令も届かない可能性がある。
過負荷保護には位置の再指令で復帰する条件があるため、原因を除去する前に運転指令を繰り返さない。

## 検証範囲と資料

ソフトウェアの検証対象は、DMA受信・フレーム照合・読戻し不一致・無応答・位置モード確認・
目標設定後のトルクON順序・ID/baud変更ツールの通信手順。実機での応答、設定の永続化、
負荷時の電源降下、1Mbps波形、機構の方向・干渉範囲は実機確認が必要。

- [STS3215-C018メーカー仕様書（秋月配布）](https://akizukidenshi.com/goodsaffix/STS3215-C018.pdf)：電源、電流、位置分解能、通信、保護条件。
- [秋月製品ページ](https://akizukidenshi.com/catalog/g/g130969/)：対象型番。
- [Waveshare公式SDK配布ページ](https://docs.waveshare.com/Bus_Servo_Adapter_A/Resources-And-Documents)：STServo Python SDKの`sms_sts.py`と通信実装によりレジスタ、baud符号化、SYNC_WRITE形式を照合。
- [Waveshare ST3215資料](https://www.waveshare.com/wiki/ST3215_Servo)：出荷時ID、複数台のID設定。
- [基板リファレンス](board_serial_svmd.md)・[デバイスプロトコル](device_protocol.md)。
