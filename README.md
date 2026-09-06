# キャチロボ2026 同志社大学 チーム ちぷ☆すた 制御リポジトリ
キャチロボバトルコンテスト2026のチーム ちぷ☆すたの制御コードベースです。
## 技術スタック
### STM32 シリーズ
多くの基板は STM32 シリーズのマイコンを搭載している。STM32 シリーズでは STM32 VSCode Extension v3.x + C++ で原則開発する。

### RA4M1 シリーズ
一部の基板は Renesas の R7FA4M1AB3CFM (通称 R4 マイコン) を搭載している。R4 マイコンでは、 PlatformIO + C++ で原則開発する。

### ROS2（archived）
`ros2/` は archived。開発・ビルド検証の対象外で、参照用に残している。
機体座標、手動速度制御、操作権と設定反映は `host/`（Rust）が担当する。

基板FWはデバイス制御と安全停止を担う汎用実行基盤とし、機体の軸名、機械換算、
入力割当、サーボIDは`host/config/*.toml`に置く。構成の詳細は
[汎用ファームウェア設計](docs/generic_firmware.md)を参照。

## ドキュメント
機体・回路・制御のドメイン知識は [docs/](docs/README.md) を参照。

全基板の駆動・読取り・通信を個別に切り替える実機テストは
[基板ネットワークの動作テスト](docs/firmware_tests.md)を参照。
各基板のペリフェラルを単体で動かして確認するコードは [samples/](samples/README.md) にある。

HAL 非依存ロジックのホスト側テストは [tests/](tests/README.md) にある。

## 手動操縦と模擬接続

```sh
cargo run --locked --manifest-path host/Cargo.toml --bin host -- --simulate
```

GUIは操縦・調整・診断・文書の4画面。起動済みhostには`hostctl`から接続できる。
操作権、停止復帰、設定保存の手順は[host操作ガイド](docs/host_operation.md)、
開発時の責務分担と拡張先は[hostアーキテクチャ](docs/host_architecture.md)を参照。
