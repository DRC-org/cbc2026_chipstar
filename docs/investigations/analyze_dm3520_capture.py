"""保存したz低速試験の再解析。標準ライブラリのみ。実機には接続しない。"""
import csv
import math
from pathlib import Path
import statistics


def slope(rows, time_key):
    times = [float(row[time_key]) for row in rows]
    positions = [float(row["measured"]) for row in rows]
    mt, mp = statistics.mean(times), statistics.mean(positions)
    return sum((t - mt) * (p - mp) for t, p in zip(times, positions)) / sum(
        (t - mt) ** 2 for t in times
    )


def main():
    path = Path(__file__).with_name("dm3520_low_cap.csv")
    with path.open() as source:
        rows = list(csv.DictReader(source))
    for label, selected in [("all_RUN_samples", rows), ("interior_samples_3_to_minus2", rows[3:-2])]:
        rate = slope(selected, "board_seconds")
        host_rate = slope(selected, "host_first_seen_seconds")
        print(f"{label}: n={len(selected)}, rate={rate:.9f}, "
              f"rate/0.025={rate / .025:.9f}, host_clock_rate={host_rate:.9f}")
    gear = 3591 / 187
    print(f"gear=3591/187={gear:.12f}")
    for label, factor in [
        ("output_rad", 1), ("rotor_rad", gear), ("electrical_rad", gear * 7)
    ]:
        k = 2 * math.pi / 72 * factor
        print(f"{label}: native_per_mm={k:.12f}, "
              f"display_for_real_10mm_at_k11.730445={10 * k / 11.730445:.9f}")


if __name__ == "__main__":
    main()
