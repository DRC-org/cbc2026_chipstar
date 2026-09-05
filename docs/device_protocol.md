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

応答の先頭語は、能力通知が `DEVICE`、状態通知が `STATE`、正常応答が `OK`、拒否が
`ERR` である。未知のフィールドを受信側が読み飛ばせるよう、応答の値は
`key=value` 形式にする。

```text
DEVICE protocol=1 board=cctl slots=3 watchdog_ms=250
ERR code=BAD_COMMAND
ERR code=OUT_OF_RANGE
```

## cctl

| 指令 | 意味 |
|---|---|
| `ENABLE <mask> <0|1>` | スロットの有効状態を変更 |
| `HOME <mask>` | 指定スロットの現在位置を原点にする |
| `TARGET <slot> <value>` | ネイティブ単位の目標位置を設定 |

`mask` のbit 0..2はslot 0..2に対応する。`TARGET` はRUN中だけでなくSAFE中にも
受理できるが、出力はRUNへ遷移するまで有効にならない。

```text
STATE t=12345 mode=RUN en=7 a0=1.250/1.230 a1=-40.000/-39.500 a2=0.500/0.490 err=00
```

各 `aN` は `目標値/実測値`。単位は能力表で定義したネイティブ単位である。

## serial_svmd

| 指令 | 意味 |
|---|---|
| `SERVO ENABLE <id> <0|1>` | トルクを切り替える |
| `SERVO TARGET <id> <position> <speed> <accel>` | 位置指令を送る |
| `SERVO READ <id>` | 現在位置を取得する |

IDは1..253、positionは0..4095とする。範囲外の指令はサーボへ送らない。

## svmd

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
