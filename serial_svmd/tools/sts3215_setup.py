"""STS3215の単体設定。USB_Servo(J16)またはTTL半二重アダプタへ直接接続する。"""
import argparse
import time

BAUDS = (1_000_000, 500_000, 250_000, 128_000, 115_200, 76_800, 57_600, 38_400)


def packet(servo_id, instruction, data=b""):
    body = bytes([servo_id, len(data) + 2, instruction]) + bytes(data)
    return b"\xff\xff" + body + bytes([(~sum(body)) & 255])


class ServoBus:
    def __init__(self, port):
        self.port = port

    def send(self, servo_id, instruction, data=b""):
        self.port.reset_input_buffer()
        frame = packet(servo_id, instruction, data)
        if self.port.write(frame) != len(frame):
            raise RuntimeError("送信が完了しませんでした")
        self.port.flush()

    def receive(self, servo_id, count):
        end = time.monotonic() + 0.15
        buf = bytearray()
        while time.monotonic() < end:
            buf += self.port.read(max(1, self.port.in_waiting))
            while len(buf) >= 6:
                if buf[:2] != b"\xff\xff" or buf[2] > 253 or not 2 <= buf[3] <= 66:
                    del buf[0]
                    continue
                n = buf[3] + 4
                if len(buf) < n:
                    break
                frame = bytes(buf[:n])
                del buf[:n]
                if (sum(frame[2:]) & 255) != 255 or frame[2] != servo_id:
                    continue
                if frame[4]:
                    raise RuntimeError(f"ID {servo_id}: サーボ異常 0x{frame[4]:02X}")
                if frame[3] != count + 2:
                    continue
                return frame[5:-1]
        raise TimeoutError(f"ID {servo_id}: 有効な応答なし（ID・baud・電源・SWCTLを確認）")

    def read(self, servo_id, address, count=1):
        self.send(servo_id, 2, bytes([address, count]))
        return self.receive(servo_id, count)

    def ping(self, servo_id):
        self.send(servo_id, 1)
        self.receive(servo_id, 0)

    def write(self, servo_id, address, data):
        # SYNC_WRITEを1台宛に使い、設定変更前IDで返るACKとの競合を避ける。
        self.send(254, 0x83, bytes([address, len(data), servo_id]) + bytes(data))
        time.sleep(0.02)

    def verified_write(self, servo_id, address, data):
        self.write(servo_id, address, data)
        if self.read(servo_id, address, len(data)) != bytes(data):
            raise RuntimeError(f"ID {servo_id}: レジスタ{address}の読戻しが一致しません")

    def inspect(self, servo_id):
        model = int.from_bytes(self.read(servo_id, 3, 2), "little")
        ident, baud = self.read(servo_id, 5, 2)
        mode = self.read(servo_id, 33)[0]
        torque = self.read(servo_id, 40)[0]
        pos = int.from_bytes(self.read(servo_id, 56, 2), "little")
        voltage, temperature = self.read(servo_id, 62, 2)
        print(f"id={ident} model={model} baud_code={baud} mode={mode} torque={torque} "
              f"position_raw={pos} voltage={voltage / 10:.1f}V temperature={temperature}C")

    def configure(self, servo_id, action, value):
        self.verified_write(servo_id, 40, b"\x00")
        self.verified_write(servo_id, 55, b"\x00")  # EPROM unlock
        if action == "set-id":
            self.write(servo_id, 5, bytes([value]))
            servo_id = value
            if self.read(servo_id, 5) != bytes([value]):
                raise RuntimeError("新IDの読戻し不一致")
        elif action == "set-baud":
            self.write(servo_id, 6, bytes([BAUDS.index(value)]))
            self.port.baudrate = value
            if self.read(servo_id, 6) != bytes([BAUDS.index(value)]):
                raise RuntimeError("新通信速度での読戻し不一致")
        elif action == "position-mode":
            self.verified_write(servo_id, 33, b"\x00")
        self.verified_write(servo_id, 55, b"\x01")
        self.inspect(servo_id)
        print("読戻し一致。再通電後に同じID・通信速度でinspectし、保存を確認してください。")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--port", required=True)
    p.add_argument("--baud", type=int, choices=BAUDS, default=1_000_000)
    p.add_argument("--id", type=int, choices=range(254), default=1, metavar="0..253")
    p.add_argument("--single-servo", action="store_true", help="バスに1台だけ接続したことを確認して設定変更を許可")
    sub = p.add_subparsers(dest="action", required=True)
    sub.add_parser("inspect")
    scan = sub.add_parser("scan")
    scan.add_argument("--last-id", type=int, choices=range(254), default=20, metavar="0..253")
    change_id = sub.add_parser("set-id")
    change_id.add_argument("value", type=int, choices=range(1,254), metavar="1..253")
    baud = sub.add_parser("set-baud")
    baud.add_argument("value", type=int, choices=BAUDS)
    sub.add_parser("position-mode")
    args = p.parse_args()
    if args.action not in ("scan", "inspect") and not args.single_servo:
        p.error("設定変更は1台だけ接続し --single-servo を指定してください")
    import serial
    with serial.Serial(args.port, args.baud, timeout=0.01, write_timeout=0.2, exclusive=True) as port:
        bus = ServoBus(port)
        if args.action == "scan":
            for ident in range(args.last_id + 1):
                try:
                    bus.ping(ident)
                    print(f"応答 ID={ident}")
                except TimeoutError:
                    continue
                except RuntimeError as error:
                    print(error)
        elif args.action == "inspect":
            bus.inspect(args.id)
        else:
            try:
                bus.configure(args.id, args.action, getattr(args, "value", None))
            except (RuntimeError, TimeoutError) as error:
                raise SystemExit(f"設定途中で失敗: {error}。変更後のID/baudになっている可能性があります。再探索して確認してください。") from error


if __name__ == "__main__":
    main()
