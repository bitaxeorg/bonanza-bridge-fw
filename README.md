# bonanza-bridge-fw for bitaxeBonanza-1004x

The RP2040 is a UART bridge between the ESP32-S3 and four BZM2 ASICs. This
branch targets **1004x**, with a board-1004 image manifest and protocol **2.0**.
The ESP32 owns fan PWM/tach, ASIC reset/trip, 5 V enable, TPS546 control and
voltage ADCs. The RP2040 never configures the former board-control GPIOs.

The ESP data interface retains `[low eight bits, bit 8]` TX byte pairs and raw
8-bit RX bytes. Independent tasks handle both directions. PIO1 samples complete
9N1 words at 5 Mbaud and DMA drains RX into an aligned 1024-word ring. RX starts
at boot without a board-control lease. The receiver requires an idle-high level
before its first start bit and a stop/idle level after every word, so a steady
low unpowered input does not generate a stream of zero bytes.

| Interface | Peripheral | RP2040 pins | Format | Baud |
|---|---|---|---|---|
| Read-only diagnostics | UART0 | TX GPIO0, RX GPIO1 | 8N1 | 115200 |
| ESP32 data | UART1 | TX GPIO4, RX GPIO5 | 8N1 | 2000000 |
| ASIC data | PIO1 | TX GPIO8, RX GPIO9 | 9N1 | 5000000 |

ESP32 data TX GPIO17 connects to RP2040 RX GPIO5; ESP32 RX GPIO18 connects to
RP2040 TX GPIO4. The ASIC net named `RX_3V3` is driven by RP2040 GPIO8; the net
named `TX_3V3` is received on GPIO9. No USB interface is used on the RP2040.

A three-second watchdog is fed once a second by the async executor. It recovers
a stalled bridge; board power/reset/trip handling remains the ESP32's job. See
[1004x control ownership](docs/safety-interlock.md).

## Build and test

Install stable Rust and the RP2040 target:

```sh
rustup target add thumbv6m-none-eabi
```

Build the firmware and complete flash images:

```sh
cargo +stable build --release --locked
./scripts/build-images.sh
```

The image script also requires `arm-none-eabi-objcopy` and `elf2uf2-rs` on PATH.
It writes `build/bonanza-bridge-fw-1004x.bin` and `.uf2`. It does not overwrite the
older `build/bonanza-rp2040-firmware.bin` if present. Each image contains boot2,
Cortex-M0+ vectors and a CRC-protected 96-byte manifest identifying board 1004,
protocol 2.0 and the build version. The default version includes the package
version, Git revision and `.dirty` when applicable; `BONANZA_BRIDGE_FW_VERSION`
can provide a release version.

Run host unit tests explicitly for your host target (the Cargo default is the
embedded target):

```sh
cargo +stable test --lib --target aarch64-apple-darwin --locked
```

Use `rustc +stable -vV` to find the host triple on other systems.

## Program through Anvil

First install the SWD-enabled `bitaxe-raw-bonanza` ESP32 firmware, version 0.0.2
or newer. On 1004x ESP32 GPIO14 is SWCLK and GPIO15 is the RP2040 SWDIO. The
separate display SWDIO on GPIO16 is not used.

From the Anvil checkout:

```sh
python3 Anvil.py --bridge-probe
python3 Anvil.py --flash-bridge ~/Bitcoin/ESP-Miner/bonanza-bridge-fw/build/bonanza-bridge-fw-1004x.bin
```

Anvil's main menu also offers `8) RP2040 bridge firmware`. It disables TPS output
and 5 V, asserts ASIC reset, verifies each flash page by readback and resets the
RP2040 only after the complete image CRC matches. Programming leaves board power
off. A reset/re-enumeration is required before resuming ASIC use.

Anvil requires this checkout's new **1004x** image. Earlier bridge images either
require an RP2040-owned board-control lease or use a different RX encoding.

## Read-only diagnostic UART

UART0 is retained for firmware identity and RX loss counters. The current
ESP32 raw firmware does not proxy this UART onto USB; Anvil board commands go
directly to the ESP32. These commands are useful to external UART tools and
future diagnostics. There is no boot banner on the ASIC data stream.

Requests are exactly `[6, 0, ID, 0, 0, command]`. Replies are
`[length LE16, ID, payload...]`.

- `0x01`: `[schema=1, major=2, minor=0, version length, ASCII version...]`.
- `0x02`: `[schema=1, FIFO loss count LE32, ring overwrite count LE32]`.

Malformed requests, old GPIO/fan commands and old lease commands return `0x11`.
A stalled partial packet returns `0x10`; a reply stalls for at most 100 ms.
Protocol 2 intentionally prevents old 1002 ESP-Miner hosts from interpreting
this UART-only bridge as their board-control interlock.

## Bench verification

This 1004x build passed 24 host logic tests and a release build. Anvil programmed
the connected RP2040 through the ESP32, read back all 30,808 image bytes, matched
CRC32 `FDBF2C9D`, and completed target reset. The programmed BIN SHA256 was
`e305990d370fac6485a89510f0ebc446de6baf60aa6920709d9592095df7d719`.

Afterward SWD still returned DPIDR `0x0BC12477`, and ESP32 status confirmed ASIC
reset asserted, 5 V/TPS off, no trip, fan command full and zero domain taps.
Actual bridge startup/UART traffic and BZM2 operation have not been directly
verified with ASIC power off. Flash readback and SWD availability alone do not
establish transport functionality.
