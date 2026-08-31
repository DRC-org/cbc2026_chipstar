# clangd (LSP) の設定

C/C++ の補完・定義ジャンプ・診断は clangd で行う。クロスコンパイルするプロジェクトが
多いため、そのままでは標準ライブラリを解決できない。必要な設定をまとめる。

## 仕組み

clangd は `compile_commands.json`（コンパイルデータベース）から、各ソースファイルの
コンパイル引数を読み取る。これは **ビルドの副産物** なので、一度ビルドするまで
clangd は何も解決できない。

| プロジェクト | 生成方法 | 出力先 |
|---|---|---|
| `cctl` / `serial_svmd` | `cmake --preset Debug` | `build/Debug/compile_commands.json` |
| `tests` | `cmake --preset default` | `build/compile_commands.json` |
| `svmd` | `pio run -t compiledb` | `compile_commands.json`（プロジェクト直下） |

各プロジェクトの `.clangd` がこの場所を指している。`svmd` の
`compile_commands.json` は生成物なので追跡していない。

## クロスコンパイラのシステムインクルード

`compile_commands.json` にはコンパイラのパスが入っているが、clangd は
**そのコンパイラを実行してよいと明示されない限り**、システムインクルードのパスを
コンパイラに問い合わせない。許可がないと組み込み向けの推測に失敗し、
`'cstdint' file not found` のようなエラーが全ファイルに出る。

許可は `--query-driver` で与える。これは clangd のコマンドライン引数でのみ
指定できる。リポジトリ内の `.clangd` からは設定できない — 任意のバイナリを
clangd に実行させられてしまうため、意図的に塞がれている。

VS Code では各プロジェクトの `.vscode/settings.json` に置いてある。

```jsonc
// cctl / serial_svmd
"clangd.arguments": [
    "--query-driver=/opt/st/stm32cubeclt_*/GNU-tools-for-STM32/bin/arm-none-eabi-*,/usr/bin/arm-none-eabi-*"
]

// svmd
"clangd.arguments": [
    "--query-driver=${userHome}/.platformio/packages/toolchain-*/bin/arm-none-eabi-*"
]
```

STM32 側は STM32CubeCLT 同梱のツールチェーンと、ディストリビューションの
`gcc-arm-none-eabi` の両方を許可している。ツールチェーンを別の場所に入れている
場合は、このグロブに自分のパスを足す。

`tests` はホストの `g++` でビルドするため `--query-driver` は要らない。

## 他のエディタを使う場合

`--query-driver` は clangd の起動引数なので、エディタごとに指定方法が違う。
上記と同じグロブを、そのエディタの clangd 起動引数へ渡せばよい。

## VS Code で診断が二重に出る場合

`cctl` には STMicroelectronics 版の clangd 拡張（`stm32cube-ide-clangd`）の設定も
残っている。この拡張と `llvm-vs-code-extensions.vscode-clangd` を同時に有効化すると、
2 つの LSP が同じファイルを解析して診断が重複する。どちらか一方を無効化する。
