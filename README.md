# bitaxe-raw usbserial firmware

bitaxe-raw is usbserial passthrough firmware for talking directly to ASICs and board peripherals over USB. This `pico` version supports the RP2040 (like in the RPi Pico dev board). The asic UART has been moved to PIO1 to support 9bit serial frames for the Intel BZM2 ASIC.

This branch is targeting the [bitaxeBIRDS](https://github.com/bitaxeorg/bitaxebirds) BZM2 dev board.

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
The usbserial firmware will create two serial ports. The first serial port is "control serial" for I2C, GPIO, and ADC. The second serial port is "data serial" and is pass through UART.

### Data Serial
- Second serial port
- **9-bit serial (9N1)**: 9 data bits, no parity, 1 stop bit
- All data is passed through bidirectionally
- USB serial baudrate is mirrored to the 9-bit UART output. Baudrates up to 5Mbaud have been tested, and seem to work 🤞

**9-bit Data Encoding over USB:**

Data is sent/received as pairs of bytes:
- **First byte**: Lower 8 bits of the 9-bit word (bits 0-7)
- **Second byte**: Bit 8 (only LSB is used, can be 0 or 1)

Examples:
- To send `0x155` (binary: `1_01010101`): Send bytes `[0x55, 0x01]`
- To send `0x0AA` (binary: `0_10101010`): Send bytes `[0xAA, 0x00]`
- Received 9-bit data is sent to USB in the same format

**Note:** The 9th bit can be used for addressing or protocol-specific purposes depending on your ASIC requirements.


### Control Serial
- First serial port
- baudrate does not matter

**Packet Format**

| 0      | 1      | 2  | 3   | 4    | 5   | 6... |
|--------|--------|----|-----|------|-----|------|
| LEN LO | LEN HI | ID | BUS | PAGE | CMD | DATA |

```
0. length low
1. length high
	- packet length is number of bytes of the whole packet. 
2. command id
	- Whatever byte you want. will be returned in the response 
3. command bus
	- always 0x00 
4. command page
	- I2C:  0x05
	- GPIO: 0x06
	- ADC:  0x07
5. command 
	- varies by command page. See below
6. data
	- data to write. variable length. See below
```

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
