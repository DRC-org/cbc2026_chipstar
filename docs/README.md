# docs

キャチロボ2026 機体・回路・制御のドメイン知識。

## 開発

- [ai_workflow.md](ai_workflow.md) — AI エージェントの作業分割とコミットの規約
- [clangd.md](clangd.md) — clangd(LSP) の設定、compile_commands.json の生成、クロスコンパイラの扱い

## 基板

- [board_cctl.md](board_cctl.md) — cctl(STM32G474) のクロック、ペリフェラルとハンドル、ピン割当、ハード固有の注意点
- [board_serial_svmd.md](board_serial_svmd.md) — serial_svmd(STM32F303K8T6) のクロック、ペリフェラルとハンドル、ピン割当、サーボ通信回路

## 機体・制御

- [rtheta_z_machine.md](rtheta_z_machine.md) — rθz 3軸機構の構成、各軸の駆動系、座標定義、原点方針、要実測パラメータ
- [cctl_can_bus.md](cctl_can_bus.md) — cctl(STM32G474) の FDCAN クロック/ビットレート、ピン割当、バス用途分離、CAN ID 衝突回避設計
- [motor_protocols.md](motor_protocols.md) — DM-S3519 / RobStride EL05 / M3508+C620 の CAN プロトコル要点

## 資料

- `rulebook_vol16.pdf` — 競技ルールブック
