# bitaxe-raw-bonanza firmware

bitaxe-raw-bonanza is RP2040 passthrough firmware for communicating with ASICs and board peripherals from a ESP32S3. The asic UART has been moved to PIO1 to support 9bit serial frames for the Intel BZM2 ASIC.

This branch is targeting the [bitaxeBonanza-1002x](https://github.com/bitaxeorg/bitaxeBonanza/tree/1002x)

## Developing

Install Rust:

```Shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

rustup target add thumbv6m-none-eabi

cargo install probe-rs-tools --locked
cargo install elf2uf2-rs --locked
cargo install cargo-binutils
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
elf2uf2-rs target/thumbv6m-none-eabi/release/firmware firmware.uf2

# Convert and deploy the UF2 image to an mounted RP2040:
elf2uf2-rs -d target/thumbv6m-none-eabi/release/firmware
```

## Running
The RP2040 bonanza firmware exposes two hardware UART interfaces to the ESP32S3 and one PIO-based 9-bit UART to the ASIC. USB serial is not used by this firmware.

### Serial Interfaces

| Interface | RP2040 Peripheral | RP2040 Pins | Format | Baudrate | Purpose |
|-----------|-------------------|-------------|--------|----------|---------|
| Control Serial | UART0 | TX: GPIO0, RX: GPIO1 | 8N1 | 115200 | Fan control and board control commands |
| Data Serial | UART1 | TX: GPIO4, RX: GPIO5 | 8N1 | 5000000 | ESP32S3-side ASIC data stream |
| ASIC Serial | PIO1 | TX: GPIO8, RX: GPIO9 | 9N1 | 5000000 | BZM2 ASIC-side data stream |

### Data Serial
- Uses RP2040 UART1 on GPIO4/GPIO5.
- 8N1 on the ESP32S3 side and 9N1 on the BZM2 ASIC side.
- All data is passed through bidirectionally.
- The firmware currently uses a fixed baudrate of 5000000 on both sides.

**8-bit to 9-bit Data Encoding:**

Data is sent/received as pairs of bytes:
- **First byte**: Lower 8 bits of the 9-bit word (bits 0-7)
- **Second byte**: Bit 8 (only LSB is used, can be 0 or 1)

Examples:
- To send `0x155` (binary: `1_01010101`): Send bytes `[0x55, 0x01]`
- To send `0x0AA` (binary: `0_10101010`): Send bytes `[0xAA, 0x00]`
- Received 9-bit data is sent to ESP32 in the same format

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
	- I2C:  0x05
	- GPIO: 0x06
	- ADC:  0x07
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
- `0x12`: buffer overflow
- `0xFF`: command-specific string error

**Packet Timing**

- Control packets should be sent as a continuous byte stream.
- A partial control packet that stalls for more than a few milliseconds is treated as a timeout error.

**I2C**

Commands:

- write: 0x20
- read: 0x30
- readwrite: 0x40

Data:

- [I2C address, (bytes to write), (number of bytes to read)]

Example:

- write 0xDE to addr 0x4F: `08 00 01 00 05 20 4F DE`
- read one byte from addr 0x4C: `08 00 01 00 05 30 4C 01`
- readwrite two bytes from addr 0x32, reg 0xFE: `09 00 01 00 05 40 32 FE 02`

**GPIO**

Commands:

- pwr_en: 0x00
- 5v_en: 0x01
- asic_rst: 0x02
- asic_trip (read-only): 0x03

Data:

- [pin level] (omit for read operations)

Example:

- Set pwr_en High: `07 00 00 00 06 00 01`
- Set 5v_en High: `07 00 00 00 06 01 01`
- Get asic_rst: `06 00 00 00 06 02`
- Get asic_trip: `06 00 00 00 06 03`

**ADC**

Commands:

- read domain1: 0x50
- read domain2: 0x51
- read domain3: 0x52

Example:

- read domain1: `06 00 00 00 07 50`
- read domain2: `06 00 00 00 07 51`

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
