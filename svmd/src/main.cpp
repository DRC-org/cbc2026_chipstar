#include <Arduino.h>
#include <Arduino_CAN.h>
#include <Servo.h>

constexpr uint32_t CAN_ID = 0x300;
constexpr uint8_t SV0_PIN = 3;
constexpr uint8_t SV1_PIN = 6;
constexpr uint8_t SV2_PIN = 10;
constexpr uint8_t SV3_PIN = 9;

Servo servo0;
Servo servo1;
Servo servo2;
Servo servo3;

void log_every_n(const uint32_t n, const String &str) {
    static uint32_t counter = 0;
    counter++;
    if (counter % n == 0) {
        Serial.println(str);
    }
}

bool read_can_data(uint64_t *out_command, uint64_t *out_value, uint64_t *out_omake) {
    CanMsg const msg = CAN.read();
    int id = msg.id; // IDを取得
    int length = msg.data_length;
    log_every_n(3, String(id) + ", " + String(length));

    if (id != CAN_ID || length != 8) {
        return false;
    }

    uint32_t command = msg.data[0];
    uint32_t value = static_cast<uint32_t>(msg.data[1]) << 24 | static_cast<uint32_t>(msg.data[2]) << 16 |
                     static_cast<uint32_t>(msg.data[3]) << 8 | static_cast<uint32_t>(msg.data[4]);
    uint32_t omake = static_cast<uint32_t>(msg.data[5]) << 16 | static_cast<uint32_t>(msg.data[6]) << 8 |
                     static_cast<uint32_t>(msg.data[7]);

    *out_command = command;
    *out_value = value;
    *out_omake = omake;

    return true;
}

void setup() {
    Serial.begin(115200);

    servo0.attach(SV0_PIN);
    servo1.attach(SV1_PIN);
    servo2.attach(SV2_PIN);
    servo3.attach(SV3_PIN);

    // サーボに角度を送信
    servo0.write(0);
    servo1.write(0);
    servo2.write(0);
    servo3.write(0);

    // CANの初期設定（周波数は1MHz）
    CAN.begin(CanBitRate::BR_1000k);
}

void loop() {
    static uint64_t command = 0;
    static uint64_t value = 0;
    static uint64_t omake = 0;

    // CAN受信かつid一致で受信データ読み出し
    if (CAN.available()) {
        // CANメッセージを読み取る
        if (read_can_data(&command, &value, &omake)) {
            // CANで受信した値を0～270度に変換
            uint32_t angle = value * 270 / std::numeric_limits<uint32_t>::max();
            log_every_n(1, "Angle: " + String(angle));

            // 0～270度をサーボのマイクロ秒に変換
            const uint32_t scaled_angle =
                static_cast<uint32_t>(static_cast<float>(angle) * 180 / 270.0f); // 0～180度にスケーリング

            // サーボに角度を送信
            servo0.write(scaled_angle);
            servo1.write(scaled_angle);
            servo2.write(scaled_angle);
            servo3.write(scaled_angle);
        } else {
            log_every_n(10, "Invalid CAN message received");
        }
    }

    // if (Serial.available()) {
    //     angle = Serial.parseInt();
    //     servo0.write(angle);
    //     servo1.write(angle);
    //     servo2.write(angle);
    //     servo3.write(angle);

    //     while (Serial.available()) {
    //         Serial.read();
    //     }
    // }

    delay(10);
}
