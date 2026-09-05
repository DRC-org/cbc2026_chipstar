# 基板ネットワークの動作テスト

`host` の `fw_test` は、cctl・svmd・serial_svmd・dcmdの汎用FWを使用して、
駆動・実測値の読取り・通信を機能別にON/OFFする対話型ツール。
ゲームパッド、GUI、機体プロファイルは不要。FWや `app.cpp` をテストごとに差し替えない。

## 接続と起動

通常のhostやシリアルモニタは終了する。1つのシリアルポートには本ツールだけを接続する。
配線変更は電源OFFで行い、電源電圧・信号レベル・GND・コネクタ極性・CAN終端を
各基板の資料で確認する。出力テスト前に機構の可動範囲と非常停止手段を確保する。

```sh
cd host
cargo run --bin fw_test -- --board cctl --serial-device /dev/ttyACM0
cargo run --bin fw_test -- --board svmd --serial-device /dev/ttyACM0
cargo run --bin fw_test -- --board dcmd --serial-device /dev/ttyACM0
cargo run --bin fw_test -- --board serial-svmd --serial-device /dev/ttyUSB0
```

上記は対象ごとに1つずつ起動する。svmd/DCMDはcctlのFDCAN2を経由する。
serial_svmdはUSART2のUSBシリアル変換器へ直接接続し、既定38400 baud。
それ以外は既定115200 baud。必要なら `--baud-rate` で指定する。
CANは1 Mbps、コネクタは [cctl](board_cctl.md)、
[serial_svmd](board_serial_svmd.md)、[DCMD](board_dcmd.md) を参照。

起動時に接続先の能力を照会し、対象基板へSTOPを送る。
svmd/DCMDはCAN先の応答も確認し、応答がなければ出力テストを開始せず終了する。
起動時のSTOPは対象基板全体に作用する。svmd/DCMDのテスト時はゲートウェイの
cctlの3軸も停止・無効化するため、稼働中の機体には接続しない。

serial_svmdはSTOP/SAFE時に有効設定と保持目標を解除するFWを使用すること。
古いFWから更新した後は、テストの選択にFW再書込みは不要。

## 操作

初期状態は全テストOFF。`on` で開始し、`off` で機能単位に終了する。
出力目標は必ず明示する。値は機体座標ではなく基板のネイティブ単位。

| 対象 | 機能名 | ONの例 | 確認対象 |
|---|---|---|---|
| cctl | `motor0` / `motor1` / `motor2` | `on motor0 0.1` | EL05 / M3508 / DMの配線・駆動。slot 0/2はrad、slot 1はモータ角deg |
| cctl | `status` | `on status` | USB経由のSTATE、目標/実測値、モータのerror bits |
| svmd | `motor0`〜`motor3` | `on motor0 1500` | 各PWM出力とサーボ。500〜2500 us |
| svmd | `status` | `on status` | CAN返信、出力有効mask、指令パルス幅 |
| serial_svmd | `motor1`〜`motor253` | `on motor1 2048` | 指定IDのSTS3215駆動。位置0〜4095、速度100・加速度10 |
| serial_svmd | `read1`〜`read253` | `on read1` | 指定IDの実測位置・トルク有効状態・エラー |
| DCMD | `motor0` | `on motor0 50` | PWM0/J10。±100 permille（±10%）以内 |
| DCMD | `encoder` | `on encoder` | ENC1/J6の符号付きカウント・X信号の累積回数 |
| DCMD | `status` | `on status` | CAN返信、モード、有効状態、実際に適用されたduty |
| 全基板 | `communication` | `on communication` | 能力照会/応答またはCANの定期疎通と受信行の表示 |

`off motor0`、`off encoder` などで個別にOFFにする。
複数出力は独立して有効化できるが、初回の配線確認は1出力ずつ行う。
serial_svmdは同時最大16出力・16読取り。FWが保持できるIDは起動から最大16種類で、
別のID群に変更する場合は基板を再起動する。読取りは1周期1IDずつ順番に行う。

出力は既定5秒で自動OFF。`--seconds 10` のように1〜30秒で変更できる。
同じ出力への `on` は目標値と期限を更新する。自動OFF時は残っている出力番号を表示する。
これはhostの指令状態であり、実機停止の保証を表す表示ではない。

`stop` は対象基板の全出力と能動的なテストを停止する。
`status` / `encoder` の受信表示は継続する。svmdの `status` は返信を得るための
定期照会を伴う。`quit` または標準入力EOFではSTOP送信を試みて終了する。
Ctrl+C・プロセス強制終了・通信切断時はFWの250ms Watchdogに依存する。

`off communication` は疎通テストと全受信ログ表示だけをOFFにする。
出力維持に必要な通信や有効な読取りは継続する。物理通信の遮断ではない。

## 配線確認例：DCMD

```text
on communication
on status
on encoder
on motor0 50
off motor0
on motor0 -50
off motor0
off encoder
quit
```

まずモータOFFのまま軸を手で回し、カウントが両方向で増減すること、X信号の回数が
変化することを確認する。その後、低出力で回転方向とカウントの符号を確認する。
カウントは4逓倍の生値で、回転数・距離への換算は行わない。
DCMDには立上りのランプ制御と反転待機があるため、目標dutyへの到達は即時ではない。
DCMDのOFFはブレーキであり、電源遮断や惰性回転ではない。

## 通信断停止の確認

出力をONにしてから `watchdog` を入力すると、対象への送信（読取りを含む）を500ms停止する。
その間も状態の受信表示は続ける。終了後にSTOPを送り、host側の出力設定も解除する。
自動RUNはしない。テスト中の新規ON/OFFは拒否し、`stop` で中断できる。

DCMD/svmdでは `result=2` の通知、cctlでは `mode=STOP`、serial_svmdでは
テスト後に再び `on read<ID>` を指定して状態を確認する。
500ms後には明示STOPも送るため、停止した事実だけではWatchdogの成功を判定できない。
通知の時刻や実際の停止タイミングも確認する。serial_svmdには自発的なタイムアウト通知がない。

応答は配線診断の材料であり、自動PASS判定ではない。特にsvmdのパルス幅は指令値であり、
サーボの実測位置ではない。DCMDのdutyも電流・回転の実測ではない。
コネクタ先までの配線確認には、実際の動きや必要に応じた波形測定を併用する。

## 自動テスト

```sh
cd host
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

初期OFF、独立切替、自動OFF、入力範囲、CAN符号化、ENC復号、読取りの巡回、
Watchdogテスト中の無送信と再始動抑止を検証する。実機の電気的な検証は含まない。
