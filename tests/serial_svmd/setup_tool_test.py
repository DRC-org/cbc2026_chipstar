"""実サーボへ接続せず、設定ツールの送信先変更と保存の読戻しを検証する。"""
import contextlib
import importlib.util
import io
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("setup_tool", Path(__file__).parents[2] / "serial_svmd/tools/sts3215_setup.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Port:
    def __init__(self):
        self.regs = bytearray(256)
        self.regs[5] = 1
        self.regs[40] = 1
        self.baudrate = 1_000_000
        self.rx = bytearray()
        self.history = []

    @property
    def in_waiting(self):
        return len(self.rx)

    def reset_input_buffer(self):
        self.rx.clear()

    def flush(self):
        pass

    def read(self, count):
        data = self.rx[:count]
        del self.rx[:count]
        return data

    def write(self, packet):
        self.history.append(packet)
        assert sum(packet[2:]) & 255 == 255
        assert self.baudrate == module.BAUDS[self.regs[6]]
        if packet[4] == 0x83:
            assert packet[7] == self.regs[5]
            address, count = packet[5:7]
            self.regs[address:address + count] = packet[8:8 + count]
        else:
            assert packet[2] == self.regs[5]
            address, count = packet[5:7]
            data = self.regs[address:address + count]
            self.rx += module.packet(self.regs[5], 0, data)
        return len(packet)


class SetupTests(unittest.TestCase):
    def test_id_change_uses_new_address_and_relocks(self):
        port = Port()
        with contextlib.redirect_stdout(io.StringIO()):
            module.ServoBus(port).configure(1, "set-id", 3)
        self.assertEqual(port.regs[5], 3)
        self.assertEqual(port.regs[55], 1)
        self.assertEqual(port.regs[40], 0)
        self.assertEqual(port.regs[42:48], bytes(6))

    def test_baud_change_switches_port_before_readback(self):
        port = Port()
        with contextlib.redirect_stdout(io.StringIO()):
            module.ServoBus(port).configure(1, "set-baud", 115200)
        self.assertEqual(port.regs[6], 4)
        self.assertEqual(port.baudrate, 115200)
        self.assertEqual(port.regs[55], 1)

    def test_packet_matches_known_ping(self):
        self.assertEqual(module.packet(1, 1), bytes.fromhex("FF FF 01 02 01 FB"))


if __name__ == "__main__":
    unittest.main()
