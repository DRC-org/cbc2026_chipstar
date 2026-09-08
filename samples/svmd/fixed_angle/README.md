# SVMD 固定角度サーボ確認

SVMDのPWMサーボ1台を、起動後に指定角度へ移動して保持する単体確認用FW。
CANやhostは使用しない。

## 設定

`src/main.cpp`冒頭の定数を変更する。

- `SERVO_CHANNEL`: `0=SV0`、`1=SV1`、`2=SV2`、`3=SV3`
- `SERVO_MODEL`: `ServoModel::Mg90s`または`ServoModel::Amazon20kg180`
- `TARGET_ANGLE_DEG`: 指令角度（0〜180°）

どちらも初期設定は1000〜2000usで、0〜180°へ線形換算する。MG90Sは1500usを
中央とする一般的な仕様に合わせている。Amazonの20kgサーボは正確な型番が未確定なため、
まず中央の90°を確認し、商品の仕様に合わせて`servoSpec()`の上下限を調整する。

角度と実際の軸角度の対応は個体やホーン取付方向でも変わる。最初はホーンを外すか、
機構と干渉しない状態で中央を確認する。

## ビルドと書込み

```sh
/home/kotek/.platformio/penv/bin/platformio run -d samples/svmd/fixed_angle
/home/kotek/.platformio/penv/bin/platformio run -d samples/svmd/fixed_angle -t upload
```

SVMDのUSB-CからUNO R4 Minimaへ書き込む。サーボ用5VはJ9から供給し、USB給電だけで
サーボを駆動しない。書込み後は2秒待ってから出力を開始し、内蔵LEDを点灯する。
