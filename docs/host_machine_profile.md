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
