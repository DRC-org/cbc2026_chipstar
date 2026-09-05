# host機体プロファイル

`host`はTOML形式の機体プロファイルから、機体上の軸と基板デバイスの対応を構成する。
既定値は`host/config/rtheta.toml`で、別構成は起動時に指定する。

```sh
cd host
cargo run --locked -- --machine-profile config/rtheta.toml
```

## cctlアクチュエータ

`[[axes]]`は機体単位の目標値を積分し、`native_per_unit`でcctlのslot単位へ換算する。
`input_axis`は`0=LX, 1=LY, 2=RX, 3=RY, 4=L2, 5=R2`。省略すると入力では動かない。

```toml
[[axes]]
name = "r"
unit = "mm"
slot = 0
input_axis = 1
input_sign = 1.0
speed_per_second = 100.0
native_per_unit = 0.10005072
minimum = 0.0
maximum = 120.0
initial = 0.0
```

slotは0..2で重複不可。運用範囲をFWの絶対上限内に収める。

## リミットスイッチと原点

`origin_position`は原点を採用したときにその軸へ与える機体単位の値で、既定は0。
`[axes.limit]`を書くと、cctlの接点をその軸のリミットとして使う。

```toml
origin_position = 120.0

[axes.limit]
input = 0            # 接点のbit位置。SW1=0, SW2=1, SW3=2
direction = 1.0      # スイッチへ近づく機体単位の向き
normally_closed = true
```

`input`は軸をまたいで重複できない。`direction`は`1.0`か`-1.0`。

原点出しはジョグで行う。オペレータが`direction`の向きへ軸を動かし、接点が到達した
瞬間にhostがその位置へ`origin_position`を割り当てる。目標値も同じ値へ置き直すので、
採用の前後で軸は動かない。到達している間は`direction`の向きの入力だけを捨て、
逆向きには戻せる。

`normally_closed = true`はB接点配線を表す。平常時に接点が閉じているため、押下と
断線がどちらも「到達」になり、断線しても機構へ突っ込まない側に倒れる。

`[axes.limit]`を省いた軸はスイッチを持たず、GUIの「現在位置を原点に」だけで採用する。
可動域`minimum`/`maximum`のクランプは原点を採用した軸にのみ効く。採用前に効かせると
暫定原点を基準にした範囲でスイッチまで届かなくなるためである。

## svmd PWMサーボ

`[[pwm_servos]]`を追加すると、hostはcctlのFDCAN2ゲートウェイを介してsvmdへ周期的に
SETを送る。ENABLEは明示的なRUN時だけ送り、STOPやWatchdog停止後に周期送信で
再有効化しない。`initial_us`と範囲は500..2500us内、channelは0..3で重複不可。

```toml
[[pwm_servos]]
name = "gripper"
channel = 0
input_axis = 3
input_sign = 1.0
speed_us_per_second = 500.0
minimum_us = 900
maximum_us = 2100
initial_us = 1500
enabled = true
```

## serial_svmd / STS3215

serial_svmdを使う場合は上位シリアル接続とサーボを指定する。`baud_rate`省略時は
38400。IDは1..253で重複不可、位置は0..4095、`move_speed`は0..1000、
`acceleration`は0..254とする。

```toml
[serial_svmd]
device = "/dev/ttyUSB0"
baud_rate = 38400

[[serial_svmd.servos]]
name = "arm"
id = 12
input_axis = 4
input_sign = 1.0
speed_position_per_second = 500.0
minimum_position = 1000
maximum_position = 3000
initial_position = 2000
move_speed = 400
acceleration = 30
enabled = true
```

## 安全動作

hostはプロトコルと必要なデバイス能力を確認するまでRUNを送らない。コントローラ入力の
送信が止まると各FWの250ms Watchdogが出力を切る。GUIのSpaceによるSTOPはcctl、
使用中のsvmd、serial_svmdへ配信する。
