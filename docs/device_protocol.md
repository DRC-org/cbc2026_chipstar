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
DEVICE protocol=1 board=cctl slots=3 can=2 watchdog_ms=250 params=stored
ERR code=BAD_COMMAND
ERR code=OUT_OF_RANGE
```

### DMドライバのレジスタ

DM-S3519のドライバは、CANのID `0x7FF` へ送る設定フレームでレジスタを読み書きできる。
cctlはこれを `DMREG` として中継する。デバッグアシスタント（PC＋シリアル）がなくても
CAN_ID、Master ID、制御モード、PMAX/VMAX/TMAXを設定できる。

```text
DMREG 10                  # レジスタ0x0A(CTRL_MODE)を読む
DMREG 10 00000002         # 位置速度モード(2)を書く
DMREG 21 41480000         # PMAX へ 12.5f を書く
DMREG rid=10 raw=00000002 f=2.000
```

値は32bitを8桁の16進で指定する。floatか整数かはレジスタごとに決まっており、
FWは解釈せずそのまま渡す。応答は生値とfloat解釈の両方を返すので、
どちらのレジスタでも読み取れる。

`ERR code=BUSY` はSAFEでないとき、`ERR code=CAN_TX` はFDCAN1へ送信できなかった
ときに返る。送信キューの満杯やCANドライバの状態異常でも発生するため、
このエラーだけでは電源・配線の異常を断定できない。

`DMREG` はSAFE中だけ受理する。走行中にIDやモードを変えると、指令の宛先と
フィードバックの解釈が食い違うためである。応答は非同期に届く。

`DMSTORE`はDMドライバへ現在の全パラメータの保存を要求する。
SAFE中かつ直近1秒以内にDMの無効状態を受信している場合のみ受理する。
成功判定には送信受付ではなく、ドライバの保存応答に対応する
`DMSTORE result=stored`を使う。送信失敗や条件不成立は`ERR code=DMSTORE_REJECTED`。
モータ側Flashへの書込みになるため、設定変更ごとに必要な場合だけ実行する。

DM3520の実機ではCAN ID 17の停止応答が`D0=0x11`となり、状態ビットとIDが重なった。
CAN IDは1〜15を使用する。応答先のMST_IDは別項目であり、現在の機体設定は
CAN ID 9・MST_ID 17である。モータ側ID変更は`DMREG 8`で読み戻し、保存応答を確認する。

機体設定のDM位置表現範囲は`PMAX=256 rad`、CCTL側の`dm_p_max=256`である。
DM本体にはSAFE中に`DMREG 21 43800000`で設定し、`DMREG 21`の読み戻しと
`DMSTORE result=stored`を確認する。hostのパラメータ適用だけではDM本体のPMAXは変わらない。
設定変更中は出力を停止し、本体とCCTLの値が揃ってから原点を採用する。

DM3520では内部位置が±PMAXを超えると通常の位置応答が折り返される。
その位置を位置速度モードの目標へ使うと、現在位置を保持するつもりでも遠い位置を指令する。
停止中の`DMREG 80`（内部位置）と通常の位置応答が一致することを確認する。
±256 radでは位置応答の刻みは約0.0078 rad（現在のz換算で約0.090 mm）。
この範囲は通信表現の範囲であり、機体のz可動域はhostの原点から0〜75 mmで制限する。
`slot2_min/max`はDMの絶対位置に対する別の制限で、hostの原点採用では移動しない。
モータ交換・内部原点変更・表現範囲外までの移動後は、内部位置と各範囲を再確認する。

主なレジスタ: `MST_ID`(0x07) / `ESC_ID`(0x08) / `TIMEOUT`(0x09) / `CTRL_MODE`(0x0A) /
`PMAX`(0x15) / `VMAX`(0x16) / `TMAX`(0x17) / `ACC`(0x04) / `DEC`(0x05) / `BAUD`(0x23)。

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
| `JOG <slot> <velocity>` | ネイティブ位置単位/秒の手動速度指令 |
| `CAN 2 <id> <data>` | FDCAN2へ標準IDのClassic CANフレームを送信 |
| `PARAM <id> <value>` | 実行時パラメータを設定 |
| `PARAM <id>` | 実行時パラメータを読み出す |
| `DMREG <rid> [値]` | DMドライバのレジスタを読み書き |
| `REINIT <mask>` | 指定スロットのモータ設定を入れ直す |
| `PARAMDEF` | 保存済みパラメータを消して既定値へ戻す |

`DEVICE`の`jog=1`は速度指令対応を示す。`JOG`はRUN中の有効slotだけで受理し、
Watchdogの通信期限を更新する。速度はslotごとの速度上限で制限する。
FW内の位置軌道は実測から速度の100ms分までしか先行せず、拘束中に遠い目標を蓄積しない。
非ゼロから速度0に変わった時点の実測位置を保持点にする。RUN/SAFE/STOP切り替えと
slot無効化では速度指令を破棄する。実際の停止応答はモータの位置制御とゲインに依存する。

`mask` のbit 0..2はslot 0..2に対応する。`TARGET` はRUN中だけでなくSAFE中にも
受理できるが、出力はRUNへ遷移するまで有効にならない。

有効でないslotの目標は実測位置へ追従する。トルクが切れている間に手で動かしても
そこが次の保持点になり、有効化した瞬間に元の位置へ戻ろうとすることがない。

`REINIT` はモータ側の制御モードと速度・電流制限を書き直す。モータだけ電源が
入り直すと、EL05は位置モードを、DMは位置速度モードを失うが、これらはcctlの
起動時にしか書いていないため基板を再起動するまで戻らない。`REINIT` はその
やり直しで、SAFE中だけ受理する。`OK` または `ERR code=BUSY` を返す。

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
ための経路で、復帰にはhostからの再有効化を要する。出力解除やSTOPだけではbitを
消さず、実際の位置フィードバックが戻ると解除する。明示的に再有効化されたslotは
新たな応答待ち時間で判定するが、過去の途絶bitは実受信まで保持する。

### C620のID検出とJOG拒否の診断

FDCAN1で受信した標準ID `0x201..0x208` の8バイトフレームを観測し、
1秒ごとに `C620_SCAN mask=1 configured_id=1` を送信する。
maskのbit0..7がESC ID 1..8に対応し、直近500ms以内に応答したIDだけを含む。
IDの自動変更や駆動は行わない。hostの診断画面の受信状態に検出IDを表示する。

併せて `CANSTAT bus=1` を送る。`lec=3` はACKエラーで、送信に対する
応答が得られていないことを示す。C620が1台も検出されない場合は、
ID変更より先に電源、FDCAN1（J2）の配線、CAN H/L、終端を確認する。

JOG拒否は `ERR code=JOG_REJECTED slot=1 mode=2 enabled=0 stale=2` のように返す。
modeは0=SAFE、1=RUN、2=STOP、enabledとstaleはslotごとのbit mask。
拒否時にはhostが出力を停止する。設定照合が完了していれば、その結果は維持する。

### EL05の読取り診断

CCTLは250msごとに1項目、12項目を約3秒で巡回してEL05の実設定と状態を読み出す。
成功した応答だけを次の形式で送る。hostの「診断 → 通信ログ」で
`EL05_PARAM` に絞り込むと確認できる。

```text
EL05_PARAM index=7005 raw=00000001 state=0 fault=0
```

`index` と `raw` は16進数。index 7005（run_mode）の値はrawの下位8bit、
その他はrawの32bitをIEEE 754 floatとして解釈する。
run_modeは1=位置PP、2=速度、5=位置CSP。
`state` は直近のモータフィードバックのbit23..22（0=リセット、1=校正、2=運転）。
`fault` はEL05の異常bitにCCTLの応答途絶bitを加えた値。
CCTLのRUNとモータ自身の運転状態は別に確認する。

読取り対象はrun_mode、limit_spd、limit_cur、loc_kp、loc_ref、mechPos、
mechVel、iqf、VBUS、spd_kp、spd_ki、limit_torque。
診断読取りは目標や出力状態を変更せず、位置フィードバックの鮮度も更新しない。
応答が来ない項目は出力されないため、古いログを現在値として扱わない。

## 実行時パラメータ

FWを書き直さずに実機調整を終えられるよう、調整対象の定数はhostから変更できる。

cctlは変更を基板のFlash最終ページへ書き戻すので、電源を入れ直しても詰めた値の
まま立ち上がる。ページ消去でCPUが数十ms止まるため、書き戻すのはSAFE中に、
最後の変更から1秒空いてからまとめて1回だけである。内容が保存済みと同じときは
書かない。`DEVICE` の `params=stored` / `params=default` が保存の有無を示し、
hostはPCの機体プロファイルを正とし、保存済みの基板にも全項目を適用・照合する。
PCへの保存はGUI/APIの明示操作で行う。`PARAMDEF` で保存を消すと既定値へ戻る。

cctl以外の基板はRAMだけに保持し、電源投入で既定値へ戻る。hostは能力確認が
通った直後に機体プロファイルの値を送り直す。

```text
PARAM 4 0.70000
PARAM 4
PARAM 4 0.70000
```

PARAMの応答値は小数5桁で返す。例えば速度ゲイン `0.0005` は `PARAM 5 0.00050`
となる。STATEの位置表示（小数3桁）とは精度が異なる。

`ERR code=OUT_OF_RANGE` は範囲外か未定義のid、`ERR code=BUSY` はRUN中に
通信IDを変えようとした場合に返る。

| id | 名前 | 内容 |
|---|---|---|
| 0..2 | `m3508_pos_kp` / `_ki` / `_kd` | M3508 位置ループのゲイン |
| 3 | `m3508_max_rpm` | 位置ループが出す速度指令の上限 |
| 4..6 | `m3508_vel_kp` / `_ki` / `_kd` | M3508 速度ループのゲイン |
| 7 | `m3508_max_current_ma` | C620へ出す電流指令の上限 |
| 8 | `el05_loc_kp` | EL05 位置ループのゲイン（モータへ書き込む） |
| 9 | `el05_limit_spd` | EL05 PP速度制限（VEL_MAX 0x7024へ書き込む） |
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

## CAN基板の設定値応答

成功したPARAM SETに対し、svmdは`0x304`、DCMDは`0x314`、serial_svmdは`0x324`で
適用値を返す。DIPアドレスによるオフセットは他のフレームと共通。8 byteの内容は
`[version=1, parameter_id, 0, 0, float32のbig endian 4 byte]`。
既存の状態応答も維持する。設定に失敗した場合は適用値応答を出さない。
hostはこの値を照合し、単なるCAN送信成功や一般のOK応答を設定一致として扱わない。

### 起動時のモータ識別診断

cctlは起動時に一度、SAFE / STOP中だけFDCAN1へ識別情報の読取りを送る。
EL05のID 0..255へ通信タイプ0、続いてDMのID 0..2047へESC_IDレジスタの
読取りを20ms間隔で送り、全探索は約46秒かかる。出力有効化や設定変更は行わない。
RUNになった場合は探索を中断し、再探索にはcctlの再起動が必要となる。

```text
MOTOR_ID kind=el05 id=127
MOTOR_ID kind=dm id=9 feedback_id=10
MOTOR_RX std=1000 ext=80 last_std=513 last_ext=02007FFD scan_done=0 scan_cancel=0 tx_failed=0
```

`MOTOR_ID`は識別応答を受信した時点で通知する。DMの識別応答は位置フィードバックの
鮮度更新には使用しない。結果はhostの通信ログで確認できる。
`MOTOR_RX`は1秒ごとに、起動後の標準・拡張フレーム受信数、最後の受信ID、
探索終了・中断状態、探索フレームの送信受付失敗数を返す。`last_ext`のみ16進数。
`scan_done=1`でも`scan_cancel=1`なら全IDの探索は完了していない。
`tx_failed=0`はモータとの通信成立を保証しない。CANSTATおよび識別応答と併せて確認する。
モータの電源が切れていた期間の探索結果から、モータ不在やID不一致を断定しない。

### モータ指令の送信失敗とPIDの時間単位

CAN送信はFIFOの空きを最大2ms待ち、空かなければ失敗として返す。
STOP・SAFE・出力解除では、古い未送信指令を取り消してから停止指令を送る。
モータのEnable/Disable・周期指令・設定書込みが送信受付に失敗した場合、
cctlはSTOP・全slot無効に戻し、停止指令の再送を行う。再送成功だけでは運転を再開しない。
CANSTATの`tx_failed`は送信受付および取消の失敗累計であり、CAN上のACK数ではない。
FIFOへの受付成功はモータの動作確認ではなく、実受信の監視を引き続き使用する。

M3508のPIDは時間を秒で計算する。速度ループのKiはmA/(rpm・s)、KdはmA・s/rpm。
ミリ秒で計算する係数から移す場合はKiを1000倍、Kdを1/1000にする。
既定プロファイルはKi=0.5、Kd=0.05。既存の別プロファイルは自動換算されない。
EL05はPPモード（run_mode=1）を使用し、`el05_limit_spd`をPP用のVEL_MAXへ書き込む。
CSP用のLIMIT_SPD（0x7017）とは異なる。PP加速度はモータ側の設定を使用する。

### 電流と識別要求の送信完了

`C620_DIAG cmd_ma=... actual_ma=... rpm=...` を100ms周期で出力する。
`cmd_ma` は電流制御器が最後に生成した指令、`actual_ma` と `rpm` はESCからの
フィードバックである。指令電流が小さいまま停止している場合と、電流上限に達しても
動かない場合を区別する。送信受付と実機への反映はこの行だけでは判定しない。

`MOTOR_SCAN_TX el05_done=... dm_done=...` は識別要求のCAN送信完了イベント数。
中断も欠落もない探索ではEL05が256、DMが2048になる。
CANのACKは同じバス上の別の機器からも返るため、送信完了は対象モータの存在を
保証しない。存在確認には `MOTOR_ID` または対象モータの実際の応答を使う。
FDCAN1は自動再送を有効にする。送信受付の成功だけを見て、競合後に未送信のまま
捨てられた指令を見逃さないよう、送信完了件数も確認する。

### DMの停止中の照会と受信診断

無効なDMスロットには位置指令を送らず、無効化指令を周期送信する。
DM3520はこれに位置・状態を返すため、出力を無効にしたまま実測値を更新できる。

`DM_RX id=... data=...`は設定されたDM応答IDの8バイト受信内容を最大10Hzで表示する。
レジスタ応答と動作状態応答を区別し、停止指令後に新たな無効状態の応答があるか確認する。
