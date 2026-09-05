# docs

キャチロボ2026 機体・回路・制御のドメイン知識。

## 開発

- [ai_workflow.md](ai_workflow.md) — AI エージェントの作業分割とコミットの規約
- [clangd.md](clangd.md) — clangd(LSP) の設定、compile_commands.json の生成、クロスコンパイラの扱い
- [generic_firmware.md](generic_firmware.md) — 汎用FWとhostの責務、安全状態、基板ごとの能力
- [device_protocol.md](device_protocol.md) — hostと各基板のバージョン付き通信プロトコル
- [host_machine_profile.md](host_machine_profile.md) — 機体固有の軸、PWM/STS3215サーボ、接続先の設定

## 基板

- [board_cctl.md](board_cctl.md) — cctl(STM32G474) のクロック、ペリフェラルとハンドル、ピン割当、ハード固有の注意点
- [board_serial_svmd.md](board_serial_svmd.md) — serial_svmd(STM32F303K8T6) のクロック、ペリフェラルとハンドル、ピン割当、サーボ通信回路
- [board_dcmd.md](board_dcmd.md) — DCMDのPWM0＋ENC1構成、CAN指令と状態通知

## 実機作業

- [firmware_tests.md](firmware_tests.md) — 全基板の駆動・読取り・通信を個別にON/OFFする配線確認ツール
- [bringup.md](bringup.md) — cctl の立ち上げ手順、指令とテレメトリ、LED の読み方、調整値の場所

## 機体・制御

- [rtheta_z_machine.md](rtheta_z_machine.md) — rθz 3軸機構の構成、各軸の駆動系、座標定義、原点方針、要実測パラメータ
- [cctl_can_bus.md](cctl_can_bus.md) — cctl(STM32G474) の FDCAN クロック/ビットレート、ピン割当、バス用途分離、CAN ID 衝突回避設計
- [motor_protocols.md](motor_protocols.md) — DM-S3519 / RobStride EL05 / M3508+C620 の CAN プロトコル要点

## 資料

- [datasheets/](datasheets/README.md) — 外部部品の公式マニュアル（DM / EL05 / C620 / M3508 / ST7032 / STS3215）
- `rulebook_vol16.pdf` — 競技ルールブック
