# デバイス通信プロトコル v1

host と各基板の間で使う、改行区切りASCIIプロトコル。1行は改行を除いて128 byte
以下とし、整数は10進、実数は有限な10進表記にする。キーワードは大文字で送信する。

## 共通指令

| 指令 | 意味 |
|---|---|
| `HELLO 1` | プロトコルv1の能力照会 |
| `SAFE` | 待機状態へ遷移し出力を切る |
| `RUN` | 構成済みで有効な出力を開始する |
| `STOP` | 即時停止して出力を切る |
| `HEARTBEAT` | Watchdogを更新する |

通信期限を延ばすのは `RUN` / `TARGET` / `HEARTBEAT` だけである。`HELLO` は
接続確認のために定期送信されるので、これで期限が延びるとゲームパッドが外れて
目標指令が止まってもWatchdogが働かない。

応答の先頭語は、能力通知が `DEVICE`、状態通知が `STATE`、正常応答が `OK`、拒否が
`ERR` である。未知のフィールドを受信側が読み飛ばせるよう、応答の値は
`key=value` 形式にする。

```text
DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250
ERR code=BAD_COMMAND
ERR code=OUT_OF_RANGE
```

## 基板アドレス

svmd・DCMD・serial_svmdは、基板上のDIP（下位2bit）で0..3のアドレスを選ぶ。
CAN IDはそれぞれの基本IDへ `0x100 × アドレス` を足した値になる。
アドレス0は従来と同じIDなので、1台構成では設定不要である。

| 基板 | address 0 | address 1 | address 3 |
|---|---|---|---|
| svmd | 0x300 / 0x301 | 0x400 / 0x401 | 0x600 / 0x601 |
| DCMD | 0x310〜0x313 | 0x410〜0x413 | 0x610〜0x613 |
| serial_svmd | 0x320〜0x323 | 0x420〜0x423 | 0x620〜0x623 |

アドレスは起動時に一度だけ読む。走行中に変えると指令の宛先と応答の解釈が
食い違うため、変更したら電源を入れ直す。

hostは現在アドレス0のIDだけを送る。2台目を載せるときはhost側の対応が要るが、
hostは後から書き換えられるので、FW側だけ先に用意してある。

## 接点入力

各基板は接点入力とDIPスイッチを読み取り、状態を報告する。接点は内蔵プルアップで
受け、GNDへ閉じたときを1とする。基板が受け持つのは読み取りと10msのデバウンスまでで、
どの接点をどの機構のリミットとして使うか、到達時に何を止めるかはhostが決める。

| 基板 | 接点 | `available` | DIP | 報告の経路 |
|---|---|---|---|---|
| cctl | SW1〜SW3 | 7 | DIP1〜4 | `STATE` の `sw=` |
| serial_svmd | SW1〜SW6 | 63 | DIP1〜4 | `INPUT READ` / CAN指令 `8` |
| DCMD | SW_A〜SW_C | 7 | DIP1〜2 | CAN指令 `6` |
| svmd | なし | — | — | なし |

常時テレメトリを送るcctlは`STATE`に載せ、要求応答型の基板は問い合わせに答える。
報告するのは事実だけで、停止条件や到達のラッチは基板側に持たない。

`raw`は生値、`stable`は10msの安定値。B接点（常閉）で配線した場合、平常時は接点が
閉じてbitが1になり、押下と断線がどちらも0になる。この読み替えはhost側で行う。

DCMDの指令と応答の形式は[board_dcmd.md](board_dcmd.md)に定める。

## cctl

| 指令 | 意味 |
|---|---|
| `ENABLE <mask> <0|1>` | スロットの有効状態を変更 |
| `HOME <mask>` | 指定スロットの現在位置を原点にする |
| `TARGET <slot> <value>` | ネイティブ単位の目標位置を設定 |
| `CAN 2 <id> <data>` | FDCAN2へ標準IDのClassic CANフレームを送信 |
| `PARAM <id> <value>` | 実行時パラメータを設定 |
| `PARAM <id>` | 実行時パラメータを読み出す |

`mask` のbit 0..2はslot 0..2に対応する。`TARGET` はRUN中だけでなくSAFE中にも
受理できるが、出力はRUNへ遷移するまで有効にならない。

```text
STATE t=12345 mode=RUN en=7 a0=1.250/1.230 a1=-40.000/-39.500 a2=0.500/0.490 err=00 sw=5 stale=0
```

`err`はslotごとの異常bitをカンマ区切りで並べる。下位bitは各モータのドライバが
返す値、`0x40`は過熱、`0x80`はフィードバック途絶を表す。混ぜて1つにすると
どのモータの異常か判別できないため、slot単位で分けている。

各 `aN` は `目標値/実測値`。単位は能力表で定義したネイティブ単位である。`sw`は
SW1〜SW3の10ms安定値で、bit 0がSW1に対応する。`STATE`は50ms周期で送るので、
接点を見るための問い合わせは要らない。`sw`を持たないFWと接続した場合、hostは
接点の状態を不明として扱い、リミットとしては使わない。

`stale`はモータのフィードバックが途絶えたslotのbit maskである。RUN中に有効な
slotが`feedback_timeout_ms`を超えて応答しないと、FWはそのslotを無効化して
このbitを立てる。hostとの通信が生きていてもモータ側のCANが抜けたことを検出する
ための経路で、復帰にはhostからの再有効化を要する。

## 実行時パラメータ

FWを書き直さずに実機調整を終えられるよう、調整対象の定数はhostから変更できる。
値はRAMだけに保持し、電源投入で既定値へ戻る。hostは能力確認が通った直後に
機体プロファイルの値を送り直す。

```text
PARAM 4 0.70000
PARAM 4
PARAM 4 0.700
```

`ERR code=OUT_OF_RANGE` は範囲外か未定義のid、`ERR code=BUSY` はRUN中に
通信IDを変えようとした場合に返る。

| id | 名前 | 内容 |
|---|---|---|
| 0..2 | `m3508_pos_kp` / `_ki` / `_kd` | M3508 位置ループのゲイン |
| 3 | `m3508_max_rpm` | 位置ループが出す速度指令の上限 |
| 4..6 | `m3508_vel_kp` / `_ki` / `_kd` | M3508 速度ループのゲイン |
| 7 | `m3508_max_current_ma` | C620へ出す電流指令の上限 |
| 8 | `el05_loc_kp` | EL05 位置ループのゲイン（モータへ書き込む） |
| 9 | `el05_limit_spd` | EL05 速度制限（モータへ書き込む） |
| 10 | `el05_limit_cur` | EL05 電流制限（モータへ書き込む） |
| 11..13 | `dm_p_max` / `dm_v_max` / `dm_t_max` | DMのフレーム符号化レンジ。モータ側設定と一致必須 |
| 14 | `dm_pos_vel_limit` | DM Position-Velocityの速度上限 |
| 15..20 | `slot0_min` / `slot0_max` / `slot1_…` / `slot2_…` | slotのネイティブ単位での絶対可動域 |
| 21 | `c620_esc_id` | C620のESC ID（1..8） |
| 22..23 | `dm_can_id` / `dm_mst_id` | DMの指令IDとフィードバックID |
| 24..25 | `el05_motor_id` / `el05_host_id` | EL05の拡張IDに載るID |
| 26..29 | `m3508_period_ms` / `dm_period_ms` / `el05_period_ms` / `telemetry_period_ms` | 各送信周期 |
| 30 | `watchdog_ms` | 通信期限 |
| 31 | `feedback_timeout_ms` | モータの応答が途絶えたと判断するまでの時間 |
| 32 | `m3508_max_temperature_c` | M3508の過熱と判断する温度 |

id 21..25（通信ID）の変更はSAFE中だけ受理する。走行中に宛先を差し替えると、
指令の宛先とフィードバックの解釈が食い違うためである。

検証は**FWが壊れる値だけ**を弾く。制御周期0やCAN IDの規格外は拒否するが、
強いゲインや高い電流上限は通す。書き直せない前提では、保守的な上限のほうが
詰みの原因になる。

ピン割当、CANビットレート、バッファ長、slot数は初期化とメモリ配置に埋まっており、
実行時には変更できない。

### CAN先の基板

svmd・DCMD・serial_svmdも同じ考え方で調整値を開けている。指令は各基板の
`PARAM SET`（svmdはop `4`、DCMDはop `7`、serial_svmdはop `9`）で、
byte 2 にパラメータid、byte 4..7 に float32 をbig endianで載せる。byte 3 は0。
読み出しは持たない。受理の可否は各基板の状態通知で返る。

| 基板 | id | 名前 | 内容 |
|---|---|---|---|
| svmd | 0..1 | `min_pulse_us` / `max_pulse_us` | 受理するパルス幅の範囲 |
| svmd | 2 | `watchdog_ms` | 通信期限 |
| DCMD | 0 | `max_duty` | 絶対Duty上限 [permille] |
| DCMD | 1 | `ramp_interval_ms` | 出力を1段動かす間隔 |
| DCMD | 2 | `ramp_step` | 1段あたりのDuty変化 |
| DCMD | 3 | `reverse_brake_ms` | 方向反転前にゼロを保つ時間 |
| DCMD | 4 | `watchdog_ms` | 通信期限 |
| DCMD | 5 | `pwm_frequency_hz` | PWMキャリア周波数。TIM2は8MHzで、周期tickは`8000000/Hz` |
| serial_svmd | 0 | `servo_baud` | STS3215バスのボーレート |
| serial_svmd | 1 | `servo_timeout_ms` | サーボ応答の待ち時間 |
| serial_svmd | 2 | `wait_for_write_status` | 書き込み命令の応答を待つか（0/1） |
| serial_svmd | 3 | `watchdog_ms` | 通信期限 |

`servo_baud` は、STS3215が工場出荷時1 Mbpsの個体だった場合の逃げ道である。
書き込み環境のない場所でそれに当たっても、hostから合わせられる。

STS3215の速度上限1000と加速度上限254、svmdのチャネル数は、デバイス側のプロトコルで
決まる値なので調整対象にしない。

`id`は10進の0..2047、`data`は0..8 byteを空白なしの16進表記にする。0 byteは`-`で
表す。受信フレームは次の形式で通知する。FDCAN2は1Mbps固定で、拡張IDの送信は
プロトコルv1では提供しない。

```text
CAN 2 768 0101000005DC0000
CAN_RX bus=2 id=769 data=010001000105DC00
```

CAN送信は`HELLO 1`が成功した後だけ受理する。FDCAN2を開始できなかった場合は
`ERR code=CAN_UNAVAILABLE`、送信キューへ積めなかった場合は`ERR code=CAN_TX`を返す。

## serial_svmd

| 指令 | 意味 |
|---|---|
| `SERVO ENABLE <id> <0|1>` | トルクを切り替える |
| `SERVO TARGET <id> <position> <speed> <accel>` | 位置指令を送る |
| `SERVO READ <id>` | 現在位置を取得する |
| `INPUT READ` | 接点入力とDIPの状態を返す |

IDは1..253、positionは0..4095とする。範囲外の指令はサーボへ送らない。
speedは0..1000、accelは0..254。最大16個のIDをRAM上に保持し、RUN中に250ms以上
有効な指令が途切れると全サーボのトルクを切る。

STOP・SAFE・Watchdog停止では保持していた有効設定と目標値も解除する。
再始動にはTARGETとENABLEを再設定する。RUNだけでは以前の出力を復帰させない。

```text
DEVICE protocol=1 board=serial_svmd slots=16 watchdog_ms=250
SERVO_STATE id=12 position=2048 enabled=1 error=00
INPUT_STATE raw=3 stable=1 dip=5 available=63
```

### CAN

同じ指令をcctlのFDCAN2からも受ける。機体としてはこちらを使い、PCへのUSBは
cctlの1本にまとめる。USART2のASCIIは基板単体で触るための口として残す。
どちらの経路も同じ状態機械とWatchdogに入る。

指令は標準ID `0x320`（800）、8 byte固定。

| byte | 内容 |
|---|---|
| 0 | version=1 |
| 1 | 0=HELLO、1=SAFE、2=RUN、3=STOP、4=TARGET、5=HEARTBEAT、6=ENABLE、7=READ、8=INPUT READ |
| 2 | サーボID（1..253）。IDを取らない指令は0 |
| 3 | ENABLEの`0|1`、TARGETの加速度（0..254）。それ以外は0 |
| 4..5 | TARGETの位置（0..4095）、big endian。それ以外は0 |
| 6..7 | TARGETの速度（0..1000）、big endian。それ以外は0 |

応答は3種類。指令の受理結果は標準ID `0x321`（801）で
`[1, status, mode, servo_count, 0, 0, 0, 0]`。statusは0=OK、1=拒否、2=timeout、
modeは0=SAFE、1=RUN、2=STOP。READの応答は `0x322`（802）で
`[1, id, pos_hi, pos_lo, enabled, error, 0, 0]`。INPUT READの応答は `0x323`（803）で、
形式は接点入力の表に従う。

1バス上のserial_svmdは1台を想定する。複数台にはアドレス割当の拡張が必要。

## svmd

DCMDのCAN指令と状態通知は[board_dcmd.md](board_dcmd.md)に定める。

CAN標準ID `0x300` を指令、`0x301` を状態通知に使用する。8 byteの指令形式は次の通り。

| byte | 内容 |
|---|---|
| 0 | protocol version (`1`) |
| 1 | command (`0=STOP`, `1=SET`, `2=ENABLE`, `3=HEARTBEAT`) |
| 2 | channel (`0..3`) |
| 3 | flags / enable (`0|1`) |
| 4..5 | pulse width [us]、big endian |
| 6..7 | 予約、0 |

SETで許容するパルス幅は安全上限内に限定する。Watchdogを超過した場合は全チャネルを
detachして状態通知にtimeoutを設定する。

状態通知はCAN標準ID `0x301`、8 byteで返す。

| byte | 内容 |
|---|---|
| 0 | protocol version (`1`) |
| 1 | status (`0=OK`, `1=BAD_COMMAND`, `2=TIMEOUT`) |
| 2 | 受理したcommand |
| 3 | channel |
| 4 | 有効チャネルのbit mask |
| 5..6 | 対象channelのpulse width [us]、big endian |
| 7 | 予約、0 |
