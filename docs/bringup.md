# 汎用FW立ち上げ手順

機体としての起動・調整は[host操作ガイド](host_operation.md)に従う。
JOG対応のcctl FWを使い、PCの設定との一致を確認してから運転する。

## 接続

cctlのFDCAN1にEL05、M3508/C620×2台を、FDCAN2に周辺基板を接続する。
CANは1Mbps、終端抵抗とGND共有を確認する。PCへの接続はcctlのUSB CDC。
STS3215のバス速度はサーボの設定に合わせる。全UARTを同一速度と仮定しない。

## 手動確認

1. SAFEで実測位置と設定の一致を確認する。
2. 原点調整モードで低速操作し、r・zのリミットとθの基準姿勢を確認する。
3. 停止して原点を採用する。機体の原点採用はモータのゼロ書換えではない。
4. スティック中立で運転再開し、軸ごとに少しずつ操作する。
5. 正方向、移動量、停止後の保持、可動域と干渉を確認して設定を保存する。

通常の停止・保持と、出力を切るSAFE/STOP、物理非常停止を区別する。
再通電や通信断の後は、設定・原点・中立を確認して明示的に再開する。

## 基板単体の端末確認

hostを終了してシリアルポートを解放した状態で行う。

```text
HELLO 1
SAFE
ENABLE 1 1
RUN
JOG 0 0.02
JOG 0 0
STOP
```

JOGまたはHEARTBEATを継続しない場合はWatchdogで停止する。
SAFE中のTARGETは非アクティブslotの実測追従で置き換わるため、
SAFE中に位置目標を仕込んでからRUNする手順には依存しない。
詳細は[デバイスプロトコル](device_protocol.md)を参照。

## 書き込み

ST-LINK/V2 を cctl の J1 へ繋ぎ、STM32CubeCLT の CLI で書き込む。

```sh
cd cctl
cmake --build --preset Debug
/opt/st/stm32cubeclt_*/STM32CubeProgrammer/bin/STM32_Programmer_CLI \
  -c port=SWD mode=UR -w build/Debug/DRC-CCTL2026.elf -v -rst
```

`mode=UR`（Under Reset）で接続する。書き込み後は USB CDC が再列挙されるので、
`/dev/ttyACM*` の番号が変わることがある。

書き込みだけなら USB CDC 側のケーブルは不要だが、動作確認には両方繋ぐ。

## 実機で残る確認

クロスビルドと単体テストでは、配線、CAN終端、モータの正方向、実際の換算係数、
負荷時のゲイン、非常停止後の物理状態は確認できない。初回は低出力・単軸で確認し、
機構上のストッパへ到達する前に停止できる作業領域を確保する。
