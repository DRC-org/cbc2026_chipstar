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
| 診断 | 実機・模擬接続の切替、受信状態、再初期化、通信ログ、模擬障害 |
| 文書 | 配線ガイド、操作手順などのMarkdown文書 |

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

### キーボード操作

GUIにフォーカスがある間、次の操作が使える。画面右上の「キー操作」からも一覧を開ける。

| キー | 操作 |
|---|---|
| Esc / Space | 停止・保持 |
| Ctrl+Enter | 運転再開 |
| F1 / F2 / F3 / F4 | 操縦 / 調整 / 診断 / 文書へ切替 |
| h / l | 前 / 次のタブ（端では循環） |
| j / k | 下 / 上へスクロール（長押し可） |
| Ctrl+d / Ctrl+u | 下 / 上へ半ページ移動 |
| gg / Shift+g | ページの先頭 / 末尾（ggは0.8秒以内） |
| Ctrl+Shift+Enter | 調整画面の編集内容を一時適用 |
| Ctrl+S | 調整画面で適用中の設定を保存 |
| F12 | キー操作一覧を開閉 |

文字・数値の入力中はEsc以外のショートカットを抑止する。Escは入力中でも停止を要求する。
運転・保存などはキーの長押しで繰り返さず、開始と停止が同時に入力された場合は停止を優先する。
操作権や運転状態の条件は画面のボタンと共通で、AI操作中も停止を実行できる。

調整画面は、未適用の編集と適用中の設定の保存状態を別々に表示する。
編集中の内容が適用中の設定と異なる間は、GUIの保存ボタンとCtrl+Sを無効にする。
保存するには先に一時適用するか、「適用中の内容に戻す」で編集を破棄する。
操作結果は画面下部に表示し、長いメッセージはマウスを重ねて全文を確認できる。
診断画面の通信ログは入力した文字列で絞り込める。

折り畳み項目は初期状態で展開する。調整値には単位と説明を併記する。
「読込元・保存先」にパスを指定して「ファイルから読込」を押すと編集欄に読み込む。
機体への反映は「一時適用」、ディスクへの書き込みは「保存」で行う。
保存先を変更して保存が成功すると、以後はそのパスを使用する。

診断の接続設定から実機・模擬接続を切り替えられる。切替は停止中に行い、原点を再確認する。
AI操作中は「通常操縦へ戻す」で停止・保持して操作権を解除できる。自動では運転再開しない。

文書画面は`docs/`以下の`.md`を一覧化し、見出し・表・リスト・コードブロックなどを描画する。
文書追加や編集後は「一覧・本文を再読込」で反映する。Markdown間の相対リンクは同じ画面で開く。
Mermaidのコードブロックはソース表示となる。

### 操縦の開始

認識対象はDualSense / DualSense Edge。複数候補がある場合は自動選択せず、使用する
1台を接続する。操縦画面では停止中に「DualSense」と「画面操作」を選択できる。
画面操作は実機・模擬接続ともに利用でき、運転再開後に各軸の±ボタンを押している間だけ
低速20%で動く。ボタンを離す、ページを切り替える、ウィンドウからフォーカスを外すと
入力が0になる。更新が150ms途切れた場合も0になる。模擬接続では画面操作を初期選択する。

1. 設定の一致確認が終わるまで待つ。
2. 調整画面で原点を採用する。原点まで移動する場合は原点調整モードで低速操作する。
3. 軸の正方向、原点、暫定可動域、干渉を実機で確認する。
4. 原点調整モードを解除し、スティックを中立にして運転再開する。

「停止・保持」は速度を0にし、位置保持を続ける。診断の「全出力停止」「SAFE」は
出力を切る。物理非常停止は別系統で動力電源を切る。
通信断、DualSense操作中のコントローラ切断、基板や原点の異常では出力停止となり、自動再開しない。

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
| `save` | `--token [--text 保存先パス]` | 適用中の機体設定を保存。保存先省略時は現在の設定ファイル |
| `connection` | `--token --file connection.toml` | `serial_device`と`baud_rate`、任意の`simulate`を指定して再接続 |
| `reinit` | `--token --axis` | モータの制御モードを再設定、原点無効化 |
| `fault` | `--token --text disconnect/reconnect/reject` | 模擬接続の障害注入 |

APIは同じOSユーザだけが接続できるUnixソケット。既定は
`$XDG_RUNTIME_DIR/catchrobo-host.sock`、未設定時は`$HOME/.cache/catchrobo/host.sock`。
両プログラムの`--socket`で変更できる。ネットワークへは公開しない。

通信形式は、UTF-8 TOML本文の長さを4 byteのbig endianで前置する。1接続1要求、本文上限
128KiB。要求の構造は`host/src/application/command.rs`の`Request`、応答は`Reply`。
`ok`は受付結果、`data`は状態または設定のTOML文字列。状態取得は操作権不要。

## ソフトウェアでの確認

```sh
cargo test --locked --offline --manifest-path host/Cargo.toml
cmake --build tests/build
ctest --test-dir tests/build --output-on-failure
```

結合テストは別プロセスの模擬hostへ実際のソケットで接続し、操作権、入力期限切れ、
停止、再接続、原点の再確認、一時適用と保存、基板の拒否を確認する。
