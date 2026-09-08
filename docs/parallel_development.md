# 並行開発

Git worktree は、Git の履歴を共有しながら別の作業ディレクトリでブランチを編集する機能。
変更ごとに worktree、ブランチ、担当エージェントを割り当てる。
エージェントを複数起動するだけでは作業ファイルは分離されないため、各エージェントの作業ディレクトリを指定する。

## 作業ディレクトリの作成

元のリポジトリで実行する。以下は `host-ui` と `fw-control` という独立した変更の例。

```sh
git status --short
git worktree list
git worktree add -b codex/host-ui ../catchrobo2026-workspaces/host-ui main
git worktree add -b codex/fw-control ../catchrobo2026-workspaces/fw-control main
```

各エージェントは割り当てられたディレクトリで作業し、開始時に
`pwd`、`git branch --show-current`、`git status --short` を確認する。
同じブランチを複数の worktree でチェックアウトすることはできない。
担当範囲と、共有インターフェースに変更があるかを開始時に決める。

開始点は `main` のコミット。元のディレクトリの未コミット変更や未追跡ファイルは引き継がれない。
必要な変更が別ブランチにある場合は、そのブランチを開始点にする。
実機用の未コミット設定が必要な場合は内容を確認し、必要な設定ファイルだけを個別に反映する。

Codex アプリでは、新規タスクの実行先に Worktree を選ぶ方法も使える。
アプリ管理の worktree と手動作成した worktree は、どちらも同じビルド手順で扱える。
アプリで作成した作業をコミット・統合する前に、専用ブランチを作成する。

## ビルドとテスト

各 worktree のルートで実行する。

```sh
make build-host
make test
```

基板 FW の変更では、対象に応じて `make build-cctl`、`make build-dcmd`、
`make build-serial-svmd`、`make build-svmd` も実行する。
`make test` の FW テストは PC 上のロジック検証であり、実機動作の確認は別途行う。

`host/target/`、各基板の `build/`、`tests/build/`、PlatformIO の `.pio/` は
各 worktree で生成する。生成済みディレクトリをコピーしたり、共通ディレクトリにリンクしたりしない。
Cargo の出力先を変更する `CARGO_TARGET_DIR` は共有先に設定せず、既定の出力先を使う。
並列ビルドでメモリが不足する場合は、例えば `CARGO_BUILD_JOBS=2 CMAKE_BUILD_PARALLEL_LEVEL=2 make test` とする。

## シミュレータの同時起動

host と hostctl は Unix ソケットというローカル通信の接続先を使う。
既定の接続先はユーザ単位で共通なので、同時起動時は worktree ごとに異なる `SOCKET` を指定する。

それぞれの worktree のターミナルで、次を実行する。

```sh
export SOCKET="$PWD/.cache/host.sock"
make host-sim
```

GUI が不要なら `make host-sim-headless` を使う。
別のターミナルも同じ worktree に移動して、同じ接続先を指定する。

```sh
export SOCKET="$PWD/.cache/host.sock"
make status
make stop
make estop
```

`stop` は停止・保持、`estop` はソフト緊停で、host プロセスは終了しない。
終了するには GUI を閉じるか、GUI なしで起動したターミナルで Ctrl+C を押す。
`.cache/` は Git の追跡対象外。ソケットの親ディレクトリは host が作成する。
Unix ソケットにはパス長制限があるため、深いディレクトリで起動に失敗する場合は、
ユーザ所有の短いパスに変更し、各 host と hostctl に同じ値を指定する。

`SOCKET` を省略した場合は、従来どおり host の既定接続先を使う。
実機接続の `make host` と `make host-headless` も `SOCKET` を受け付ける。
バイナリを直接実行する場合は `--socket` を指定する。

## 統合と終了

各担当は対象のテストを通し、担当変更だけをコミットする。
統合担当は元のリポジトリで、他のエージェントが編集・Git 操作をしていないことと、
未コミット変更がないことを確認してから、変更を一つずつ取り込む。
未コミット変更がある場合は所有者と整理し、勝手に stash や破棄をしない。

```sh
git switch main
git merge codex/host-ui
make test
git merge codex/fw-control
make test
```

worktree は編集途中の干渉を防ぐが、統合時の競合や機能間の不整合は解決しない。
同じファイルやプロトコルに触れた変更は、差分を確認して調整し、統合後にも検証する。

取り込み済みで作業ディレクトリがクリーンであることを確認し、手動作成した worktree を終了する。

```sh
git worktree remove ../catchrobo2026-workspaces/host-ui
git branch -d codex/host-ui
```

未追跡・無視対象のファイルにも必要な設定やログがないか確認する。
削除を拒否された場合は残存ファイルを確認し、強制削除で回避しない。
アプリ管理の worktree はアプリの管理操作で扱う。

## 実機検証

USB、CAN 接続先、書込み装置、機体は worktree を分けても共有される。
実機接続・FW 書込みは統合担当の一つの作業ディレクトリに集約する。
他の worktree はシミュレータで検証し、実機検証は接続先と対象コミットを確認して順番に行う。
