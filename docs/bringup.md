# 汎用FW立ち上げ手順

基板FWを書き込んだ後、hostから能力確認、目標設定、有効化、RUNの順で立ち上げる。
cctlとserial_svmdは電源投入時にSAFEとなる。svmdは全PWM出力無効で起動し、
hostは明示的なRUN時に必要なチャネルを有効化する。

## 事前確認

- cctlのFDCAN1にはEL05、M3508/C620、DMを接続し、FDCAN2にはsvmdを接続する。
- CANは終端抵抗、GND共有、1Mbpsを確認する。
- serial_svmdの上位UARTは38400 bps、STS3215側UARTは115200 bpsである。
- 機体固有の換算、可動域、入力割当、サーボIDをhostの機体プロファイルに設定する。

設定形式は[host_machine_profile.md](host_machine_profile.md)、バス構成は
[cctl_can_bus.md](cctl_can_bus.md)を参照。

## hostの起動

```sh
cd host
cargo run --locked -- --serial-device /dev/ttyACM0 \
  --machine-profile config/rtheta.toml
```

serial_svmdを使う場合、そのデバイス名とボーレートは機体プロファイルに記述する。
GUIのステータス画面でcctlとserial_svmdの能力確認を確認する。svmdはcctlのFDCAN2
ゲートウェイ経由で接続される。

## cctlを端末から確認する

USB CDCは改行区切りASCIIなので、GUIを使わず端末からも確認できる。

```text
HELLO 1
DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250
```

主な指令は次のとおり。

| 指令 | 動作 |
|---|---|
| `HELLO 1` | プロトコルと能力を確認 |
| `SAFE` | 待機状態へ移り出力を切る |
| `RUN` | 有効化済みslotの出力を開始 |
| `STOP` | 即時停止して出力を切る |
| `HEARTBEAT` | Watchdogを更新 |
| `ENABLE <mask> <0\|1>` | slotの有効状態を変更 |
| `HOME <mask>` | 指定slotの現在位置を原点にする |
| `TARGET <slot> <value>` | slotのネイティブ単位で目標値を設定 |
| `CAN 2 <id> <data>` | FDCAN2へ標準CANフレームを送信 |

`mask`のbit 0..2がslot 0..2に対応する。詳細な構文は
[device_protocol.md](device_protocol.md)を参照。

## 1軸ずつの確認

1. `HELLO 1`を送り、protocol=1と必要な能力を確認する。
2. `STATE`が50ms周期で届き、mode=SAFE、en=0であることを確認する。
3. 機構を安全な初期姿勢に置き、対象slotだけ`HOME <mask>`を送る。
4. SAFEのまま、FWのネイティブ単位で小さな`TARGET`を設定する。
5. 対象slotだけ`ENABLE <mask> 1`にし、退避可能な状態で`RUN`を送る。
6. 目標と実測の向き、換算、可動域を確認し、直ちに`STOP`できる状態を保つ。
7. `SAFE`へ戻してslotを無効化し、次のslotを確認する。

slotの能力は次のとおり。

| slot | デバイス | ネイティブ単位 | FW絶対範囲 |
|---|---|---|---|
| 0 | EL05 | rad | -12.5..12.5 |
| 1 | M3508/C620 | motor deg | -26000..26000 |
| 2 | DM-S3519 | rad | -12.5..12.5 |

向き、機械換算、運用可動域が合わない場合は`host/config/*.toml`を直す。速度・電流・
ネイティブ位置の絶対上限やデバイス制御ゲインを変更する場合だけ
`cctl/src/device_config.hpp`とFWの再書き込みが必要になる。

## テレメトリ

```text
STATE t=12345 mode=RUN en=7 a0=1.250/1.230 a1=-40.000/-39.500 a2=0.500/0.490 err=00
```

`a0..a2`は`目標/実測`で、値はネイティブ単位。host GUIでは機体プロファイルの
`native_per_unit`を使って機体単位へ戻して表示する。`err`が非0、値が非有限、または
意図しない動作があればSTOPする。

## host GUIの操作

| キー | 動作 |
|---|---|
| `Space` | 使用中の基板へSTOPを送る |
| `j` / `k` | 選択を下 / 上へ |
| `gg` / `G` | 選択を先頭 / 末尾へ |
| `Enter` | 選択中の項目を実行 |
| `gt` / `gT` | 次 / 前の画面へ |
| `1`–`4` | 画面を直接選ぶ |
| `i` | 設定画面の編集を開始 |
| `Esc` | 編集・コマンドラインを抜ける |
| `:` | cctlへ送る生の指令行を入力 |
| `?` | キー一覧 |

`RUN`はcctlと、プロファイルで使用する追加基板の能力確認が済むまで拒否される。
コントローラが切断するか送信を停止すると、250ms Watchdogにより各FWが出力を切る。

## 実機で残る確認

クロスビルドと単体テストでは、配線、CAN終端、モータの正方向、実際の換算係数、
負荷時のゲイン、非常停止後の物理状態は確認できない。初回は低出力・単軸で確認し、
機構上のストッパへ到達する前に停止できる作業領域を確保する。
