# モータ CAN プロトコル要点（DM / EL05 / M3508）

rθz 各軸で用いるモータの CAN プロトコルをまとめる。全て **1 Mbps / Classic CAN(2.0B)**。
**この文書は要点の抜粋**。一次資料は [datasheets/](datasheets/README.md) の
公式マニュアルで、記載のない詳細はそちらに当たること。
参考実装: `kotek-7/dm3520_test`, `DRC-org/edulite05-test`, `kotek-7/robo_central_controller`。
cctl 側実装は `cctl/src/{dm_motor,el05_motor,m3508_motor}.{hpp,cpp}`。

---

## z軸: Damiao DM-S3519（標準ID）

レジスタ式プロトコル。電源投入時に**位置 = 0.0 rad**。
位置・速度・トルクはいずれも**減速後の出力軸**の値。

### 制御モード（レジスタ 0x0A `CTRL_MODE`）
| 値 | モード | コマンドフレームID | ペイロード |
|---|---|---|---|
| 1 | MIT | `0x000 + ID` | pos/vel/kp/kd/tau をビット詰め |
| 2 | **Position-Velocity** | `0x100 + ID` | `P_des`(float,rad) + `V_des`(float,rad/s) |
| 3 | Velocity | `0x200 + ID` | `v_des`(float, rad/s) |

**rθz では mode 2 を採用**。目標位置 + 速度上限を渡すだけでモータ内部が位置制御する。
重力保持にも適し、電源投入 0rad が原点方針と一致。

### MIT モードのフレーム（`0x000 + ID`）
| byte | 内容 |
|---|---|
| D0 | `P_des[15:8]` |
| D1 | `P_des[7:0]` |
| D2 | `V_des[11:4]` |
| D3 | `V_des[3:0] << 4 \| Kp[11:8]` |
| D4 | `Kp[7:0]` |
| D5 | `Kd[11:4]` |
| D6 | `Kd[3:0] << 4 \| T_ff[11:8]` |
| D7 | `T_ff[7:0]` |

位置 16bit / 速度・Kp・Kd・トルク 各 12bit。位置は ±PMAX、速度は ±VMAX、
トルクは ±TMAX へ線形マッピング。**Kp は [0, 500]、Kd は [0, 5]**。

Kp=0 かつ Kd≠0 で `v_des` を与えれば定速回転、Kp=Kd=0 で `t_ff` を与えれば
トルク指定になる。**位置制御で Kd=0 にすると発振または制御不能になる。**

### 設定・特殊コマンド
- 読み出し: ID=`0x7FF`, `D0..1`=CAN_ID(LE), `D2`=`0x33`, `D3`=RID
- 書き込み: ID=`0x7FF`, `D0..1`=CAN_ID(LE), `D2`=`0x55`, `D3`=RID, `D4..7`=値(LE)
- 保存: ID=`0x7FF`, `D0..1`=CAN_ID(LE), `D2`=`0xAA`
- Enable/Disable/Zero: ID=CAN_ID, `D0..6`=`0xFF`, `D7`=`0xFC`/`0xFD`/`0xFE`

**レジスタ書込は電源断で消える。** 残すには保存コマンドが要る。

> **注意**: 読み出し・書き込みの応答は、通常のフィードバックと**同じ MST_ID** で返る。
> 見分けないと位置や温度に設定値が化けて入る。応答は `D0..1` が CAN_ID、
> `D2` が `0x33`/`0x55`/`0xAA` になる（`cctl/src/domain/dm_codec.cpp` の
> `isConfigReply`）。

### 主なレジスタ
| Addr | 名前 | 内容 | R/W |
|---|---|---|---|
| `0x04` / `0x05` | ACC / DEC | 加速度 / 減速度 | RW |
| `0x06` | MAX_SPD | 最大速度 | RW |
| `0x07` | MST_ID | フィードバックID | RW |
| `0x08` | ESC_ID | 受信ID | RW |
| `0x09` | TIMEOUT | タイムアウト警報時間 | RW |
| `0x0A` | CTRL_MODE | 制御モード [0,4] | RW |
| `0x14` | Gr | 減速比 | RO |
| `0x15` / `0x16` / `0x17` | PMAX / VMAX / TMAX | 各マッピング範囲 | RW |
| `0x19` / `0x1A` | KP_ASR / KI_ASR | 速度ループ | RW |
| `0x1B` / `0x1C` | KP_APR / KI_APR | 位置ループ | RW |
| `0x23` | baud | 通信速度 | RW |

**PMAX / VMAX / TMAX は実機から読める**（`DmMotor::requestRegister`）。
`device_config.hpp` の基板上限を推測で変更する必要はない。

### フィードバックフレーム（ID = MST_ID）
| byte | 内容 |
|---|---|
| D0 | `ID(下位4bit) \| ERR(上位4bit)` |
| D1-2 | POS 16bit（`±PMAX` へ線形） |
| D3, D4上位 | VEL 12bit（`±VMAX`） |
| D4下位, D5 | torque 12bit（`±TMAX`） |
| D6 | T_MOS（ドライバ上側 MOS の平均温度 [℃]） |
| D7 | T_Rotor（モータ内部コイルの平均温度 [℃]） |

ERR: 0=Disabled, 1=Enabled, 5=センサ読取異常, 6=パラメータ読取異常,
8=過電圧, 9=低電圧, A=過電流。

> 補足: baud > 1Mbps に設定すると自動で CAN FD になりフィードバックを受信できなくなる。1Mbps 厳守。

---

## r軸: RobStride EL05（拡張ID 29bit）

デフォルト motor ID = `0x7F`、推奨 host ID = `0xFD`。減速比 9:1 内蔵。

### 拡張ID 構造
```
bit28..24 : comm_type(5bit)
bit23..8  : data_area_2(16bit)   … 用途により host_id / tau など
bit7..0   : target_id(8bit)
```

### comm_type
| 値 | 意味 | data_area_2 | ペイロード |
|---|---|---|---|
| 0 | GetDeviceId | host_id | — |
| 1 | MotionControl(MIT) | tau(uint16) | pos/vel/kp/kd(各uint16, BE) |
| 2 | Feedback（受信） | 状態/fault/モータID | pos/vel/tau/temp |
| 3 | Enable | host_id | 0 |
| 4 | Disable | host_id | `D0`=1 で fault クリア |
| 6 | SetZero | host_id | `D0`=1 |
| 7 | SetCanId | host_id | — |
| 17 | ReadParam | host_id | `D0..1`=param_id |
| 18 | WriteParam | host_id | `D0..1`=param_id, `D4..7`=値 |
| 21 | FaultFeedback | — | — |
| 22 | SaveParam | host_id | `01 02 03 04 05 06 07 08` |

**パラメータ書込は電源断で消える。** 残すには comm_type 22 を送る。

### run_mode（`0x7005`, uint8）
| 値 | モード |
|---|---|
| 0 | 運動制御（MIT 相当） |
| 1 | 位置 (PP) |
| 2 | 速度 |
| 3 | 電流 |
| 5 | 位置 (CSP) |

### パラメータID
| ID | 名前 | 内容 | 範囲 | R/W |
|---|---|---|---|---|
| `0x7005` | run_mode | 運転モード | 上表 | RW |
| `0x7006` | iq_ref | 電流モードの目標電流 [A] | -11〜11 | RW |
| `0x700A` | spd_ref | 速度モードの目標速度 | | RW |
| `0x700B` | limit_torque | トルク制限 [N·m] | 0〜6 | RW |
| `0x7010` / `0x7011` | cur_kp / cur_ki | 電流ループ | | RW |
| `0x7016` | loc_ref | 位置モードの目標位置 [rad] | | RW |
| `0x7017` | limit_spd | 位置モードの速度制限 | | RW |
| `0x7018` | limit_cur | 電流制限 [A] | 0〜11 | RW |
| `0x7019` | mechPos | 機械角 [rad] | | R |
| `0x701A` | iqf | フィルタ後電流 [A] | | R |
| `0x701B` | mechVel | 負荷側速度 | | R |
| `0x701C` | VBUS | バス電圧 [V] | | R |
| `0x701E` | loc_kp | 位置ループ Kp | | RW |
| `0x701F` / `0x7020` | spd_kp / spd_ki | 速度ループ | | RW |
| `0x7022` | acc_rad | 速度モードの加速度 | | W |
| `0x7024` / `0x7025` | vel_max / acc_set | 位置モードの最大速度・加速度 | | W |
| `0x702E` | dcc_set | 減速度 | | RW |

### rθz での位置制御手順
`disable(clear_fault)` → `run_mode=1` → `limit_spd` / `limit_cur` / `loc_kp` 設定 →
`set_zero` → `enable` → 以降 `loc_ref` に目標位置[rad]を周期書込。

### フィードバック（comm_type=2）復号レンジ
pos `±12.57 rad`, vel `±50 rad/s`, tau `±6 N·m`（各uint16→線形）、temp = raw/10 [℃]。
拡張IDの `bit16..` に fault_bits、`bit15..8` に発信元のモータID。

---

## θ軸: DJI M3508 + C620（標準ID）

C620 ESC は**電流指令のみ**受け付ける。位置制御は cctl 側で実装する。

### コマンド（送信）
- ID = `0x200`（モータ 1〜4 を 1 フレームに集約）／`0x1FF`（5〜8）
- 各モータ int16(BE) の電流指令、スロット = `((ESC_ID-1) % 4)*2`
- スケール: **±20000 mA ↔ ±16384**（`mA * 16384 / 20000`）

> **注意**: 1 フレームが 4 台分を運ぶ。台ごとに別フレームを送ると、後から届いた
> フレームが他の台のスロットを 0 で上書きしてしまう。同じグループの台は
> 1 本にまとめてから送ること（`cctl/src/c620_group.hpp`）。

### フィードバック（ID = `0x200 + ESC_ID`）
| byte | 内容 |
|---|---|
| D0-1 | 機械角 0..8191（1回転） |
| D2-3 | 回転数 rpm(int16) |
| D4-5 | 実トルク電流(int16) |
| D6 | 温度[℃] |

### 位置制御の作り方（cctl 実装）
C620 は 1回転 0..8191 の角度しか返さないため:
1. **多回転角の積算**: 前回値との差分を ±半周でラップ補正し累積（`total_counts`）。
2. **原点**: 初回フィードバック角を 0 に採用（起動位置=原点方針）。
3. **カスケードPID**: 位置ループ（モータ多回転角[deg]誤差→目標rpm）→ 速度ループ（rpm誤差→電流mA）。
4. θ出力角 → モータ角の換算 = `θ_deg × (RING_TEETH/PINION_TEETH) × (3591/187)`。

ゲイン・電流上限は基板設定（`device_config.hpp` の `m3508` 名前空間）。実機で調整前提。
