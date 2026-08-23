# Samples

Each directory contains an `app.cpp` that can replace `/src/app.cpp`.

The normal build uses only files under `/src`. To try a sample, copy the
target sample's `app.cpp` over `/src/app.cpp`, then build the firmware.

`lcd_aqm1602.cpp` and `lcd_aqm1602.h` stay in `/src` because they are
shared by the main app and the LCD sample.
