# datasheets

機体に載っている外部部品の公式マニュアル。プロトコルの一次資料はここを見る。

[motor_protocols.md](../motor_protocols.md) は要点をまとめたもので、記載のない
詳細はこちらに当たること。

| ファイル | 部品 | 使っている場所 |
|---|---|---|
| `dm-s3519-1ec_user_manual_v1.1.pdf` | Damiao DM-S3519-1EC（DM3520-1EC ドライバ同梱） | z 軸 / `cctl/src/dm_motor.*` |
| `robstride_el05_user_manual_260713.pdf` | RobStride EL05 | r 軸 / `cctl/src/el05_motor.*` |
| `robomaster_c620_v1.01.pdf` | DJI C620 ブラシレス ESC | θ 軸 / `cctl/src/m3508_motor.*` |
| `robomaster_m3508_p19_v1.0.pdf` | DJI M3508 P19 ギアードモータ | θ 軸（機械諸元・減速比） |
| `st7032_lcd_controller.pdf` | Sitronix ST7032 LCD コントローラ | `cctl/src/lcd_aqm1602.*` の命令セット |
| `aqm1602y-nlw-fbw.pdf` | 秋月 AQM1602Y 液晶モジュール | LCD モジュールの結線・電気仕様 |
| `sts3215-c018.pdf` | Feetech STS3215 シリアルサーボ | `serial_svmd/src/sts3215.*` |
| `feetech_digital_servo_protocol_20220729.pdf` | Feetech デジタルサーボ 通信プロトコル | STS パケット仕様・レジスタ表 |

## 版について

EL05 のマニュアルは版によって内容が変わる。ここに置いてあるのは
`260713` 版。手元の実機の挙動と食い違う場合は、まず版を確認する。

DM のマニュアルは V1.1 (2024.11.18)。同じ内容で本文テキストを持たない
（画像だけの）PDF も出回っているので、検索できるこちらを正とする。
