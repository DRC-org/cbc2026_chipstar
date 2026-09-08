#include <Arduino.h>
#include <Servo.h>

constexpr uint8_t SERVO_PINS[] = {3, 6, 10, 9};

constexpr float SERVO_MIN_ANGLE = 0.0f;
constexpr float SERVO_MAX_ANGLE = 180.0f;

constexpr int SERVO_MIN_PULSE_US = 500;
constexpr int SERVO_MAX_PULSE_US = 2500;

Servo servos[4];

int angleToPulse(float angle)
{
    angle = constrain(angle, SERVO_MIN_ANGLE, SERVO_MAX_ANGLE);

    const float ratio =
        (angle - SERVO_MIN_ANGLE) /
        (SERVO_MAX_ANGLE - SERVO_MIN_ANGLE);

    return static_cast<int>(
        SERVO_MIN_PULSE_US +
        ratio * (SERVO_MAX_PULSE_US - SERVO_MIN_PULSE_US)
    );
}

void setServoAngle(float angle, uint8_t channel)
{
    servos[channel].writeMicroseconds(angleToPulse(angle));
}

void setup()
{
    Serial.begin(115200);

    servos[0].attach(SERVO_PINS[0]);
    servos[1].attach(SERVO_PINS[1]);
    servos[2].attach(SERVO_PINS[2]);
    servos[3].attach(SERVO_PINS[3]);

    setServoAngle(0.0f, 1);
    setServoAngle(0.0f, 2);
    setServoAngle(0.0f, 3);
}

void loop()
{
    if (Serial.available()) {
        float angle = Serial.parseFloat();

        if (angle >= SERVO_MIN_ANGLE &&
            angle <= SERVO_MAX_ANGLE) {

            setServoAngle(angle, 1);
            setServoAngle(angle, 2);
            setServoAngle(angle, 3);

            Serial.print("angle = ");
            Serial.println(angle);
        } else {
            Serial.println("out of range");
        }

        // 残った改行などを捨てる
        while (Serial.available()) {
            Serial.read();
        }
    }
}