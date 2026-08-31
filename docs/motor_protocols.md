# モータ CAN プロトコル要点（DM / EL05 / M3508）

rθz 各軸で用いるモータの CAN プロトコルをまとめる。全て **1 Mbps / Classic CAN(2.0B)**。
**この文書は要点の抜粋**。一次資料は [datasheets/](datasheets/README.md) の
公式マニュアルで、記載のない詳細はそちらに当たること。
参考実装: `kotek-7/dm3520_test`, `DRC-org/edulite05-test`, `kotek-7/robo_central_controller`。
cctl 側実装は `cctl/src/{dm_motor,el05_motor,m3508_motor}.{hpp,cpp}`。

---

## z軸: Damiao DM-S3519（標準ID）

レジスタ式プロトコル。電源投入時に**位置 = 0.0 rad**。

### 制御モード（レジスタ 0x0A `CTRL_MODE`）
| 値 | モード | コマンドフレームID | ペイロード |
|---|---|---|---|
| 1 | MIT | `0x000 + ID` | pos/vel/kp/kd/tau パック |
| 2 | **Position-Velocity** | `0x100 + ID` | `P_des`(float,rad) + `V_des`(float,rad/s) |
| 3 | Velocity | `0x200 + ID` | `v_des`(float, rad/s) |

**rθz では mode 2 を採用**。目標位置 + 速度上限を渡すだけでモータ内部が位置制御する。
重力保持にも適し、電源投入 0rad が原点方針と一致。

### 設定・特殊コマンド
- レジスタ書込: ID=`0x7FF`, `D0..1`=CAN_ID(LE), `D2`=`0x55`(write)/`0x33`(read), `D3`=RID, `D4..7`=値(LE)
- Enable/Disable/Zero: ID=CAN_ID, `D0..6`=`0xFF`, `D7`=`0xFC`(en)/`0xFD`(dis)/`0xFE`(zero)
- 主なレジスタ: `0x07 MST_ID`(FB ID), `0x08 ESC_ID`(受信ID), `0x0A CTRL_MODE`, `0x15 PMAX`, `0x23`(baud)

### フィードバックフレーム（ID = MST_ID）
| byte | 内容 |
|---|---|
| D0 | `ID(下位4bit) | ERR(上位4bit)` |
| D1-2 | POS 16bit（`±PMAX` へ線形） |
| D3, D4上位 | VEL 12bit（`±VMAX`） |
| D4下位, D5 | torque 12bit（`±TMAX`） |
| D6 | T_MOS（MOSFET温度） |
| D7 | T_Rotor（ロータ温度） |

ERR: 0=Disabled, 1=Enabled, 8=Overvoltage, 9=Undervoltage, A=Overcurrent 等。
POS 復号に PMAX が要る（**実機 PMAX 要確認**）。

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
| 1 | MotionControl(MIT) | tau(uint16) | pos/vel/kp/kd(各uint16, BE) |
| 2 | Feedback（受信） | 状態/fault | pos/vel/tau/temp |
| 3 | Enable | host_id | 0 |
| 4 | Disable | host_id | `D0`=1 で fault クリア |
| 6 | SetZero | host_id | `D0`=1 |
| 18 | WriteParam | host_id | `D0..1`=param_id, `D4..7`=値 |

### 主要パラメータID（WriteParam）
`0x7005 RUN_MODE`(0=MIT,1=Position,2=Velocity,3=Current), `0x7016 LOC_REF`(目標位置),
`0x7017 LIMIT_SPD`, `0x7018 LIMIT_CUR`, `0x701E LOC_KP`。

### rθz での位置制御手順
`disable(clear_fault)` → `RUN_MODE=1` → `LIMIT_SPD/LIMIT_CUR/LOC_KP` 設定 → `set_zero` →
`enable` → 以降 `LOC_REF` に目標位置[rad]を周期書込。

### フィードバック（comm_type=2）復号レンジ
pos `±12.57 rad`, vel `±50 rad/s`, tau `±6 N·m`（各uint16→線形）、temp = raw/10 [℃]。
拡張IDの `bit16..` に fault_bits。

---

## θ軸: DJI M3508 + C620（標準ID）

C620 ESC は**電流指令のみ**受け付ける。位置制御は cctl 側で実装する。

### コマンド（送信）
- ID = `0x200`（モータ 1〜4 を 1 フレームに集約）／`0x1FF`（5〜8）
- 各モータ int16(BE) の電流指令、スロット = `(ESC_ID-1)*2`
- スケール: **±20000 mA ↔ ±16384**（`mA * 16384 / 20000`）

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

ゲイン・電流上限は暫定値（`robot_config.hpp` の `m3508` 名前空間）。実機で調整前提。
