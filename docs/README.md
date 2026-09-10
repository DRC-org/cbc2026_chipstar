# docs

キャチロボ2026 機体・回路・制御のドメイン知識。

## 開発

- [host_architecture.md](host_architecture.md) — 責務分担、実行状態、依存方向、機能追加の配置

- [parallel_development.md](parallel_development.md) — worktree による作業分離、同時起動、統合
- [ai_workflow.md](ai_workflow.md) — AI エージェントの作業分割とコミットの規約
- [clangd.md](clangd.md) — clangd(LSP) の設定、compile_commands.json の生成、クロスコンパイラの扱い
- [generic_firmware.md](generic_firmware.md) — 汎用FWとhostの責務、安全状態、基板ごとの能力
- [device_protocol.md](device_protocol.md) — hostと各基板のバージョン付き通信プロトコル
- [host_machine_profile.md](host_machine_profile.md) — 機体固有の軸、PWM/STS3215サーボ、接続先の設定
- [ee_calibration.md](ee_calibration.md) — EE回転の2点較正と3:1減速時の扱い

## 基板

- [board_cctl.md](board_cctl.md) — cctl(STM32G474) のクロック、ペリフェラルとハンドル、ピン割当、ハード固有の注意点
- [board_serial_svmd.md](board_serial_svmd.md) — serial_svmd(STM32F303K8T6) のクロック、ペリフェラルとハンドル、ピン割当、サーボ通信回路
- [board_dcmd.md](board_dcmd.md) — DCMDのPWM0＋ENC1構成、CAN指令と状態通知

## 実機作業

- [status_led.md](status_led.md) — 状態表示LEDの点け方の規約
- [wiring.md](wiring.md) — 基板間の接続、コネクタ表、間違えやすい配線
- [firmware_tests.md](firmware_tests.md) — 全基板の駆動・読取り・通信を個別にON/OFFする配線確認ツール
- [bringup.md](bringup.md) — cctl の立ち上げ手順、指令とテレメトリ、LED の読み方、調整値の場所

## 機体・制御

- [pick_sequences.md](pick_sequences.md) — 取得準備・取得・搬送の実行、位置教示、工程編集
- [bonus_hand.md](bonus_hand.md) — ボーナスハンドの受け渡し位置登録、手動操作、半自動シュート

- [rtheta_z_machine.md](rtheta_z_machine.md) — rθz 3軸機構の構成、各軸の駆動系、座標定義、原点方針、要実測パラメータ
- [cctl_can_bus.md](cctl_can_bus.md) — cctl(STM32G474) の FDCAN クロック/ビットレート、ピン割当、バス用途分離、CAN ID 衝突回避設計
- [motor_protocols.md](motor_protocols.md) — DM-S3519 / RobStride EL05 / M3508+C620 の CAN プロトコル要点
- [DM3520位置単位調査](investigations/dm3520_position_units.md) — z換算の確定事項、未検証の単位、通信ログの再解析

## 資料

- [datasheets/](datasheets/README.md) — 外部部品の公式マニュアル（DM / EL05 / C620 / M3508 / ST7032 / STS3215）
- `rulebook_vol16.pdf` — 競技ルールブック

- [host_operation.md](host_operation.md) — 手動操縦、調整、AI接続、模擬接続
