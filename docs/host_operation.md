# host操作ガイド

hostが機体へのシリアル接続を一つだけ所有し、GUI・DualSense・ローカルAPIから操作する。
機体設定と座標換算はhost、周期制御と通信期限による出力停止はFWが担当する。

## 起動

リポジトリのルートで実行する。

```sh
cargo run --locked --manifest-path host/Cargo.toml --bin host -- --simulate
```

模擬接続は、設定応答、r・θ・zの位置応答、通信断、指令拒否を再現する。
干渉、負荷、モータの制動距離を再現する物理シミュレータではない。
実機では`--simulate`を外し、`--serial-device /dev/ttyACM0`を指定する。
`--headless`はGUIを開かず同じワーカーとAPIを起動する。

新しいhostの速度操作には、`DEVICE`に`jog=1`を返すcctl FWが必要。
FWのビルドは実機への書き込み・動作確認とは別の工程になる。

## 画面と基本操作

| 画面 | 内容 |
|---|---|
| 操縦 | 運転状態、停止理由、機体接続、設定一致、軸の実測位置、低速表示 |
| 調整 | 原点採用、原点調整モード、速度・可動域・基板調整値、一時適用、保存 |
| 診断 | 接続先、受信状態、再初期化、通信ログ、模擬障害、配線ガイド |

通常時は操作者表示を強調しない。AIが操作権を取得している間だけ赤枠と「AI操作中」を
表示する。状態の閲覧だけでは赤枠にならない。

| DualSense | 操作 |
|---|---|
| 左スティック上下 | r速度 |
| 左スティック左右 | θ速度 |
| 右スティック上下 | z速度 |
| L1を保持 | 低速（20%） |
| Options | 中立確認後に運転再開 |
| PS | 停止・保持 |

認識対象はDualSense / DualSense Edge。複数候補がある場合は自動選択せず、使用する
1台を接続する。模擬接続ではGUIの模擬スティックでも操作できる。

1. 設定の一致確認が終わるまで待つ。
2. 調整画面で原点を採用する。原点まで移動する場合は原点調整モードで低速操作する。
3. 軸の正方向、原点、暫定可動域、干渉を実機で確認する。
4. 原点調整モードを解除し、スティックを中立にして運転再開する。

「停止・保持」は速度を0にし、位置保持を続ける。診断の「全出力停止」「SAFE」は
出力を切る。物理非常停止は別系統で動力電源を切る。
通信断、コントローラ切断、基板や原点の異常では出力停止となり、自動再開しない。

## 起動済みhostへの接続

```sh
cargo build --locked --manifest-path host/Cargo.toml --bins
host/target/debug/hostctl status
host/target/debug/hostctl config
host/target/debug/hostctl claim
```

`claim`が返す`token`を以後の変更操作に渡す。操作権は最後の変更操作または
`heartbeat`から30秒で失効する。例の`TOKEN`は取得した値に置き換える。

```sh
host/target/debug/hostctl origin --token TOKEN --axis r
host/target/debug/hostctl origin --token TOKEN --axis theta
host/target/debug/hostctl origin --token TOKEN --axis z
host/target/debug/hostctl run --token TOKEN
host/target/debug/hostctl status
host/target/debug/hostctl input --token TOKEN --axis r --value 0.2 --seconds 1
host/target/debug/hostctl stop
host/target/debug/hostctl release --token TOKEN
```

`input`は-1..1の正規化入力。`--seconds`は0..30秒で、50ms周期に再送し最後に0へ戻す。
各軸の入力が150ms途切れると、その軸の入力が0になる。上位の通信失敗時にはFWのWatchdogも出力を停止する。
`run`の受付成功は基板の運転確認ではない。`status`の`running`と`board_mode`で確認する。

GUIとAPIは同じ操作受付を使う。AI操作中の通常操縦は停止し、`stop`はtokenなしでも
受け付ける。AIが解放・失効しても通常操縦は再開操作を待つ。

| action | 追加引数 | 用途 |
|---|---|---|
| `status` / `config` | なし | 状態・通信ログ / 現在の機体設定を取得 |
| `claim` | なし | 停止・保持してAI操作権を取得 |
| `heartbeat` / `release` | `--token` | 操作権を更新 / 解放 |
| `run` / `stop` | `run`のみ`--token` | 運転開始 / 停止・保持 |
| `safe` / `cut` | `--token` | SAFE / 全出力停止と原点無効化 |
| `origin` | `--token --axis` | 停止中の最新位置を原点として採用 |
| `adjustment` | `--token --flag true/false` | 原点調整モード |
| `input` | `--token --axis --value [--seconds]` | 速度入力 |
| `apply` | `--token --file path.toml` | 機体設定を一時適用 |
| `save` | `--token` | 適用中の機体設定を読込元へ保存 |
| `connection` | `--token --file connection.toml` | `serial_device`と`baud_rate`を指定して再接続 |
| `reinit` | `--token --axis` | モータの制御モードを再設定、原点無効化 |
| `fault` | `--token --text disconnect/reconnect/reject` | 模擬接続の障害注入 |

APIは同じOSユーザだけが接続できるUnixソケット。既定は
`$XDG_RUNTIME_DIR/catchrobo-host.sock`、未設定時は`$HOME/.cache/catchrobo/host.sock`。
両プログラムの`--socket`で変更できる。ネットワークへは公開しない。

通信形式は、UTF-8 TOML本文の長さを4 byteのbig endianで前置する。1接続1要求、本文上限
128KiB。要求の構造は`host/src/control_api.rs`の`Request`、応答は`Reply`。
`ok`は受付結果、`data`は状態または設定のTOML文字列。状態取得は操作権不要。

## ソフトウェアでの確認

```sh
cargo test --locked --offline --manifest-path host/Cargo.toml
cmake --build tests/build
ctest --test-dir tests/build --output-on-failure
```

結合テストは別プロセスの模擬hostへ実際のソケットで接続し、操作権、入力期限切れ、
停止、再接続、原点の再確認、一時適用と保存、基板の拒否を確認する。
