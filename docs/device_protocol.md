# デバイス通信プロトコル v1

host と各基板の間で使う、改行区切りASCIIプロトコル。1行は改行を除いて95 byte
以下とし、整数は10進、実数は有限な10進表記にする。キーワードは大文字で送信する。

## 共通指令

| 指令 | 意味 |
|---|---|
| `HELLO 1` | プロトコルv1の能力照会 |
| `SAFE` | 待機状態へ遷移し出力を切る |
| `RUN` | 構成済みで有効な出力を開始する |
| `STOP` | 即時停止して出力を切る |
| `HEARTBEAT` | Watchdogを更新する |
| `INPUT READ` | 接点入力とDIPの状態を返す |
| `INPUT GUARD <mask> <high>` | 停止条件とする接点と極性を設定する |

応答の先頭語は、能力通知が `DEVICE`、状態通知が `STATE`、正常応答が `OK`、拒否が
`ERR` である。未知のフィールドを受信側が読み飛ばせるよう、応答の値は
`key=value` 形式にする。

```text
DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250
ERR code=BAD_COMMAND
ERR code=OUT_OF_RANGE
```

## 接点入力

各基板は接点入力とDIPスイッチを読み取り、指定した接点を出力の停止条件にできる。
接点は内蔵プルアップで受け、GNDへ閉じたときを1とする。どの接点をどの機構の
リミットとして使うかはhost側の機体構成であり、FWは番号のまま扱う。

| 基板 | 接点 | `available` | DIP |
|---|---|---|---|
| cctl | SW1〜SW3 | 7 | DIP1〜4 |
| serial_svmd | SW1〜SW6 | 63 | DIP1〜4 |
| DCMD | SW_A〜SW_C | 7 | DIP1〜2 |
| svmd | なし | 0 | A0〜A3 |

`mask` は停止条件として監視する接点のbit、`high` はそのうち開接点を停止条件とする
bitである。B接点（NC）で断線も検出したい配線では `high` に含める。`mask` に
含まれない接点は監視だけで、状態には現れるが出力を止めない。`available` にない
bitや、`mask` の外を指す `high` は `ERR code=OUT_OF_RANGE` で拒否する。

停止条件が成立するとFWはSTOPへ遷移して出力を切り、成立をラッチする。ラッチは
接点が復帰しただけでは解除されず、`INPUT GUARD` を再送するまで残る。ラッチ中の
`RUN` は `ERR code=INPUT_ACTIVE`、RUN中の `INPUT GUARD` は `ERR code=BUSY` になる。
判定にはデバウンス前の生値を使い、10msの安定値は表示だけに用いる。設定はRAMだけに
保持し、電源再投入で監視なしへ戻る。

cctlとserial_svmdは `INPUT READ`・`INPUT GUARD` の応答を1行で返す。

```text
INPUT_STATE raw=3 stable=1 dip=5 guard=1 high=0 trip=1 available=7
```

DCMDとsvmdは同じ内容を8 byteのCANフレームで返す。IDはDCMDが `0x313`（787）、
svmdが `0x302`（770）。

| byte | 内容 |
|---|---|
| 0 | protocol version (`1`) |
| 1 | raw（生値） |
| 2 | stable（10ms安定値） |
| 3 | dip |
| 4 | guard（監視mask） |
| 5 | high（開接点で停止するbit） |
| 6 | trip（`0|1`、ラッチ状態） |
| 7 | available |

## cctl

| 指令 | 意味 |
|---|---|
| `ENABLE <mask> <0|1>` | スロットの有効状態を変更 |
| `HOME <mask>` | 指定スロットの現在位置を原点にする |
| `TARGET <slot> <value>` | ネイティブ単位の目標位置を設定 |
| `CAN 2 <id> <data>` | FDCAN2へ標準IDのClassic CANフレームを送信 |

`mask` のbit 0..2はslot 0..2に対応する。`TARGET` はRUN中だけでなくSAFE中にも
受理できるが、出力はRUNへ遷移するまで有効にならない。

```text
STATE t=12345 mode=RUN en=7 a0=1.250/1.230 a1=-40.000/-39.500 a2=0.500/0.490 err=00
```

各 `aN` は `目標値/実測値`。単位は能力表で定義したネイティブ単位である。

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

IDは1..253、positionは0..4095とする。範囲外の指令はサーボへ送らない。
speedは0..1000、accelは0..254。最大16個のIDをRAM上に保持し、RUN中に250ms以上
有効な指令が途切れると全サーボのトルクを切る。

STOP・SAFE・Watchdog停止では保持していた有効設定と目標値も解除する。
再始動にはTARGETとENABLEを再設定する。RUNだけでは以前の出力を復帰させない。

```text
DEVICE protocol=1 board=serial_svmd slots=16 watchdog_ms=250
SERVO_STATE id=12 position=2048 enabled=1 error=00
```

## svmd

DCMDのCAN指令と状態通知は[board_dcmd.md](board_dcmd.md)に定める。

CAN標準ID `0x300` を指令、`0x301` を状態通知に使用する。8 byteの指令形式は次の通り。

| byte | 内容 |
|---|---|
| 0 | protocol version (`1`) |
| 1 | command (`0=STOP`, `1=SET`, `2=ENABLE`, `3=HEARTBEAT`, `4=INPUT READ`) |
| 2 | channel (`0..3`) |
| 3 | flags / enable (`0|1`) |
| 4..5 | pulse width [us]、big endian |
| 6..7 | 予約、0 |

SETで許容するパルス幅は安全上限内に限定する。Watchdogを超過した場合は全チャネルを
detachして状態通知にtimeoutを設定する。

`INPUT READ` はbyte 2..5を0にする。svmdに外部接点入力はなく、応答はDIPだけを載せた
`0x302` のフレームで、`0x301` の状態通知は返さない。停止条件は設定できない。

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
