# Samples

Each directory contains an `app.cpp` that can replace `/src/app.cpp`.

The normal build uses only files under `/src`. To try a sample, copy the
target sample's `app.cpp` over `/src/app.cpp`, then build the firmware.

`sts3215.cpp` and `sts3215.hpp` stay in `/src` because they are used by the
main app only; samples do not depend on them.

| Sample | Purpose |
| --- | --- |
| `led` | Lights LED1-6 one by one, then all together. |
| `sw` | Lights LEDn while SWn is pressed. |
| `can_rx` | Receives CAN frames and exposes counters for Live Expressions. |
