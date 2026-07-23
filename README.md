# bonanza-bridge-fw

bonanza-bridge-fw is RP2040 passthrough firmware for communicating with ASICs and board peripherals from a ESP32S3. The asic UART has been moved to PIO1 to support 9bit serial frames for the Intel BZM2 ASIC.

This firmware targets the [bitaxeBonanza-1002x](https://github.com/bitaxeorg/bitaxeBonanza/tree/1002x).

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
counter covers a 24-hour qualification run at the measured design rate.

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

### ESP compatibility

| Bridge protocol | ESP-Miner behavior |
|---|---|
| Missing info | Rejected before board power is enabled |
| 1.0 or newer compatible minor | Supported; raw RX bytes plus `GET_RX_STATS` |
| Major version other than 1 | Rejected before board power is enabled |

The Bonanza MVO ESP firmware requires protocol 1.0 or newer within major
version 1. It verifies the info schema and raw RX stats command before board
power is enabled.

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
