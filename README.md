# bonanza-bridge-fw

bonanza-bridge-fw is RP2040 passthrough firmware for communicating with ASICs and board peripherals from a ESP32S3. The asic UART has been moved to PIO1 to support 9bit serial frames for the Intel BZM2 ASIC.

This firmware targets the [bitaxeBonanza-1002x](https://github.com/bitaxeorg/bitaxeBonanza/tree/1002x).

The local output interlock, host responsibilities, failure behavior, and
hardware limitations are documented in
[Bridge safety interlock](docs/safety-interlock.md).

## Developing

Install Rust:

```Shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

rustup target add thumbv6m-none-eabi

cargo install probe-rs-tools --locked
cargo install elf2uf2-rs --locked
cargo install cargo-binutils
```

Run the host-visible policy and protocol unit tests explicitly, because the repository's default Cargo target is the RP2040:

```Shell
cargo test --lib --target x86_64-unknown-linux-gnu
```

For SWD-based development and debugging:

```Shell
# Build the latest firmware:
cargo build --release

# Build, program, and attach to the device with RTT for debugging:
cargo run --release

# Just flash the device, don't attach to RTT:
cargo flash --release --chip RP2040

# Erase all flash memory:
probe-rs erase --chip RP2040 --allow-erase-all
```

For UF2-based development:

```Shell
# Build the latest firmware:
cargo build --release

# Convert the ELF to an RP2040-compatible UF2 image:
elf2uf2-rs target/thumbv6m-none-eabi/release/bonanza-bridge-fw bonanza-bridge-fw.uf2

# Convert and deploy the UF2 image to an mounted RP2040:
elf2uf2-rs -d target/thumbv6m-none-eabi/release/bonanza-bridge-fw
```

## Running
The bonanza-bridge-fw firmware exposes two hardware UART interfaces to the ESP32S3 and one PIO-based 9-bit UART to the ASIC. USB serial is not used by this firmware.

### Serial Interfaces

| Interface | RP2040 Peripheral | RP2040 Pins | Format | Baudrate | Purpose |
|-----------|-------------------|-------------|--------|----------|---------|
| Control Serial | UART0 | TX: GPIO0, RX: GPIO1 | 8N1 | 115200 | Fan control and board control commands |
| Data Serial | UART1 | TX: GPIO4, RX: GPIO5 | 8N1 | 2000000 | ESP32S3-side BIRDS-compatible data stream |
| ASIC Serial | PIO1 | TX: GPIO8, RX: GPIO9 | 9N1 | 5000000 | BZM2 ASIC-side data stream |

### Data Serial
- Uses RP2040 UART1 on GPIO4/GPIO5.
- 8N1 on the ESP32S3 side and 9N1 on the BZM2 ASIC side.
- ESP-to-ASIC data is passed through as 9-bit words. ASIC-to-ESP data forwards
  the low eight response bits as raw bytes, matching the BIRDS receive path.
- Each direction runs in an independent async task. Paced DMA drains ASIC RX
  continuously into a naturally aligned 1024-word memory ring, so executor,
  interrupt, ESP command, or UART drain latency cannot stall the eight-entry
  PIO RX FIFO.
- The ASIC side remains fixed at 5000000 baud. The ESP link uses 2000000 baud,
  providing more than twelve times the measured raw receive payload budget
  while improving board-level signal margin.

**ESP32-to-ASIC 9-bit Data Encoding:**

Data sent from the ESP32S3 to the ASIC is encoded as pairs of bytes:
- **First byte**: Lower 8 bits of the 9-bit word (bits 0-7)
- **Second byte**: Bit 8 (only LSB is used, can be 0 or 1)

Examples:
- To send `0x155` (binary: `1_01010101`): Send bytes `[0x55, 0x01]`
- To send `0x0AA` (binary: `0_10101010`): Send bytes `[0xAA, 0x00]`
- Received ASIC data forwards only bits 0 through 7 as raw bytes. The ninth bit
  is still sampled so the receiver observes a complete 9N1 word, but it is not
  part of the BIRDS response parser contract.
- RX samples all nine bits at fixed eight-cycle intervals,
  with the first sample centered 1.5 bit periods after start detection. It
  requires the actual stop/idle level, and only then looks for the next start
  bit. Autopush occurs after all nine bits, and software then drops bit 8 before
  forwarding the byte to the ESP.

**Note:** The 9th bit can be used for addressing or protocol-specific purposes depending on your ASIC requirements.

### Control Serial
- Uses RP2040 UART0 on GPIO0/GPIO1.
- Format is 8N1.
- The firmware currently uses a fixed baudrate of 115200.

**Packet Format**

| 0      | 1      | 2  | 3   | 4    | 5   | 6... |
|--------|--------|----|-----|------|-----|------|
| LEN LO | LEN HI | ID | BUS | PAGE | CMD | DATA |

```
0. length low
1. length high
	- packet length is the number of bytes in the whole packet.
2. command id
	- Whatever byte you want. It will be returned in the response.
3. command bus
	- always 0x00
4. command page
	- System: 0x00
	- GPIO: 0x06
	- Fan: 0x09
5. command 
	- varies by command page. See below
6. data
	- data to write. variable length. See below
```

**Response Format**

Responses are also length-prefixed:

| 0      | 1      | 2  | 3... |
|--------|--------|----|------|
| LEN LO | LEN HI | ID | DATA |

- `ID` echoes the command ID from the request.
- `DATA` is the command response payload.
- Error responses use the same framing and return an error code in `DATA[0]`.

**Error Codes**

- `0x10`: timeout while receiving a packet
- `0x11`: invalid command or malformed packet
- `0x12`: safety policy denied the requested transition
- `0x13`: a safety fault is latched or the trip input is active

**Packet Timing**

- Control packets should be sent as a continuous byte stream.
- A partial control packet that stalls for more than a few milliseconds is treated as a timeout error.

**System**

Commands:

- get info: `0x01`
- get RX stats: `0x02`

The get-info response payload is:

| 0 | 1 | 2 | 3 | 4... |
|---|---|---|---|---|
| Schema version | Protocol major | Protocol minor | Version length | Version string |

- The current schema version is `1` and the control protocol version is `1.0`.
- The version string is printable ASCII and no more than 63 bytes.
- By default, builds report `<package-version>+g<short-git-sha>`, followed by `.dirty` when built from a modified checkout.
- Manufacturers can set `BONANZA_BRIDGE_FW_VERSION` at build time to use a release version instead.

ASIC RX is drained continuously by a PIO-paced DMA channel into a 1024-word
address ring, then the normal buffered UART task forwards bounded raw-byte
chunks to the ESP. This is a correctness requirement at 5 Mbaud: the eight-word
PIO RX FIFO cannot hold a complete ten-word ASIC telemetry frame while an async
executor or interrupt-disabled critical section is busy. The ring covers more
than 100 complete TDM frames, reports exact overwrite loss, and its transfer
counter covers a 24-hour qualification run at the measured design rate. The RX
state machine and DMA channel are enabled only while 5 V is on and ASIC reset is
released; every safe transition synchronously aborts DMA, drains the FIFO, and
restores the program counter to its start-bit wait before the next controlled
session. This prevents an unpowered low RX line from manufacturing bytes,
mixing powered sessions, or starving control/safety work.

Example request with command ID `0x2a`:

`06 00 2a 00 00 01`

Example response for version `1.2.3`:

`0c 00 2a 01 01 03 05 31 2e 32 2e 33`

The get-RX-stats response is fixed-width and little-endian:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | RX stats schema, currently `1` |
| 1 | 4 | Cumulative PIO RX FIFO overflow count |
| 5 | 4 | Cumulative DMA software-ring overflow count |

Example request with command ID `0x2a`:

`06 00 2a 00 00 02`

Example zero-counter response:

`0c 00 2a 01 00 00 00 00 00 00 00 00`

### Production bridge safety policy

This is a local output interlock, not a physical rail monitor or a fully
independent hardware safety controller. Its purpose is to keep the outputs
owned by the RP2040 fail-closed when the ESP stops controlling them, the ASIC
trip input asserts, or the Bridge control task stalls. See
[Bridge safety interlock](docs/safety-interlock.md) for the state machine and
host integration sequence.

The bridge always boots with these semantic requests before it accepts control commands:

- 5 V disabled
- ASIC reset asserted (`ASIC_RST/RST_N` is driven low)
- fan requested at 100 percent

The production policy is fixed in source: the local output lease is 2000 ms and
the active-high ASIC trip input is latched. There is no build-time mode that
can disable either protection. Unsafe GPIO or fan changes are denied until the
ESP arms the bridge. The ESP must then heartbeat inside the lease, and must
disarm for a healthy shutdown. Lease expiry or an asserted trip immediately
requests the safe outputs above and latches a fault; a heartbeat cannot clear
or resurrect that fault.

The RP2040 hardware watchdog is enabled with a three-second timeout and is fed
only after the control task samples trip/lease state and applies effective
outputs. UART response writes are bounded to 100 ms, and the 500 ms tach
measurement services safety at least every 10 ms. A watchdog reset restarts
the firmware into the same safe boot pin requests.

Legacy ESP firmware that does not implement protocol 1 can still request safe
outputs, but its attempt to enable 5 V is denied with `0x12`.

Safety commands use system page `0x00`:

| Command | Value | Response |
|---|---:|---|
| Get safety status | `0x10` | Current 17-byte safety status |
| Arm safety lease | `0x11` | Arms from safe outputs and returns status |
| Safety heartbeat | `0x12` | Renews an armed lease and returns status |
| Clear safety fault | `0x13` | Clears only with trip low, returns to safe-off, and returns status |
| Disarm safety lease | `0x14` | Requests safe outputs, ends a healthy lease, preserves any latched fault, and returns status |

Example get-status request with command ID `0x2a`:

`06 00 2a 00 00 10`

The response payload is fixed-width and little-endian where a field uses multiple bytes:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 1 | Status schema, currently `1` |
| 1 | 1 | Safety policy: fixed at `2` (lease plus trip-latch) |
| 2 | 1 | State: `0` safe-off, `1` controlled, `2` fault-latched |
| 3 | 1 | Fault: `0` none, `1` lease-expired, `2` ASIC-trip |
| 4 | 1 | Runtime verdict: `0` good-safe-off, `1` good-controlled, `0x80` bad-fault, `0x81` bad-lease, `0x82` bad-trip-input, `0x83` bad-unsafe-outputs |
| 5 | 1 | Production verdict: `0` good, `0x80` stage-disabled, `0x81` capability-gap, `0x82` bad-runtime |
| 6 | 2 | Capability bits |
| 8 | 2 | Evidence bits |
| 10 | 4 | Lease milliseconds remaining |
| 14 | 1 | Effective command flags: bit 0 = 5 V enabled, bit 1 = reset asserted, bit 2 = fan at 100 percent |
| 15 | 1 | Effective commanded fan percentage |
| 16 | 1 | Sampled trip input, `0` or `1` |

Capability and matching evidence bits are:

| Bit | Capability | Current firmware |
|---:|---|---|
| 0 | 5 V control path | Set |
| 1 | ASIC reset control path | Set |
| 2 | Full-fan command path | Set |
| 3 | Trip input sampled by the control task | Set |
| 4 | Independent VCORE cutoff | **Not set** |
| 5 | Autonomous fan-tach interlock | **Not set** |
| 6 | Trip monitor independent of the command task | **Not set** |

Evidence bits 0 through 3 mean outputs-safe, lease-valid, trip-clear, and
fault-clear. Evidence bits 4 through 6 mirror the three independent hardware
capabilities above. The current firmware capability value is `0x000f`, so a
healthy status reports production verdict `0x81` (capability gap). This field
is retained for protocol compatibility and must not be changed without the
corresponding hardware integration and tests.

Status reports effective command state, not physical rail measurements. A host
that requires VCORE, PGOOD, fan-tach, or temperature protection must measure and
act on those signals separately.

### ESP compatibility

| Bridge protocol | ESP-Miner behavior |
|---|---|
| Missing info | Rejected before board power is enabled |
| 1.0 or newer compatible minor | Supported; raw RX bytes plus `GET_RX_STATS` |
| Major version other than 1 | Rejected before board power is enabled |

The Bonanza MVO ESP firmware requires protocol 1.0 or newer within major
version 1. It verifies the info schema, raw RX stats command, safety status
coherence, lease, trip-clear state, reset, 5 V, full-fan command, and live fan
tach before releasing ASIC reset.

**GPIO**

Commands:

- 5v_en: 0x01
- asic_rst: 0x02
- asic_trip (read-only): 0x03

Data:

- [pin level] (omit for read operations)

Example:

- Set 5v_en High: `07 00 00 00 06 01 01`
- Get asic_rst: `06 00 00 00 06 02`
- Get asic_trip: `06 00 00 00 06 03`

The GPIO API exposes the electrical `asic_rst` pin level for compatibility.
Because this signal is `RST_N`, writing or reading `0` means reset is asserted.
Deasserting reset requires 5 V already enabled, and enabling 5 V requires reset
asserted, a live lease, and the fan command at 100 percent.

**Fan**

Commands:

- set speed: 0x10
- get tachometer: 0x20

Data:

- [speed percentage 0-100] (for set speed command)

Example:

- Set fan speed to 50%:  `07 00 00 00 09 10 32`
- Set fan speed to 100%: `07 00 00 00 09 10 64`
- Read fan tach (RPM):   `06 00 00 00 09 20`
