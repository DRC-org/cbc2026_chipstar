# tests

ファームウェアの HAL 非依存ロジックを、ホストのコンパイラで検証する。

実機もデバッガも使わずに動かせる範囲 — 単位換算、プロトコルの符号化・復号、
制御則、入力の解釈 — をここで固定する。

## 実行

```sh
cd tests
cmake --preset default
cmake --build --preset default
ctest --preset default
```

失敗した assertion だけを詳しく見たい場合は、実行ファイルを直接叩く。

```sh
./build/cctl_tests
```

## 構成

プロジェクトごとに実行ファイルを分けている。インクルードパスを対象ファームの
`src/` だけに絞ってあるため、テスト対象が HAL や他プロジェクトのヘッダに
依存し始めるとビルドが通らなくなる。依存の向きはこの仕組みで守る。

| 実行ファイル | 対象 |
|---|---|
| `cctl_tests` | `cctl/src` |
| `serial_svmd_tests` | `serial_svmd/src` |
| `svmd_tests` | `svmd/src` |
| `dcmd_tests` | `dcmd/src` |

テスト対象は各ファームの `src/domain/` に置く。`domain/` は HAL を含まない
純粋なロジックだけを入れる場所で、そこから外側（HAL ラッパやアプリ層）を
参照してはいけない。

テストフレームワークは [doctest](https://github.com/doctest/doctest) を
`vendor/doctest.h` に単一ヘッダで置いている。ビルド時のネットワーク接続は不要。

## CI

`.github/workflows/ci.yml` が push / pull request で `ctest` を実行する。
同じワークフローで `host`（Rust）と `svmd`（PlatformIO）もビルドする。

`cctl` と `serial_svmd` のファームウェアは CI でビルドしない。CubeMX が生成する
`Drivers/` と `Middlewares/` を `.gitignore` しているため、クリーンクローンでは
ソースが揃わない。両者のロジックはこのテストがホスト側で受け持つ。

## 書き方

新しいテストファイルは `tests/<プロジェクト名>/` に置き、
`CMakeLists.txt` の `add_firmware_tests` の引数に追加する。

`domain/digital_inputs.hpp` のように、全ファームが同じ内容の複製を持つヘッダの
テストは `tests/` 直下に置く。`add_firmware_tests` が全実行ファイルへ加えるため、
各ファームのコピーが同じ振る舞いをすることを、実装を共有せずに確認できる。
