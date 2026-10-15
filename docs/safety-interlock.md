# 1004x control ownership

On bitaxeBonanza-1004x the ESP32 owns ASIC_RESET (GPIO1), ASIC_TRIP (GPIO2),
5V_EN (GPIO21), fan PWM/tach (GPIO8/GPIO9), and TPS546 enable/PGOOD
(GPIO10/GPIO11). RP2040 GPIO10/11/18/20/21 are unconnected on this board and
are left at their reset input state by this firmware.

The RP2040 performs UART conversion only. It has no board-control lease,
GPIO/fan API or trip input. The old 1002 lease cannot protect the 1004 board,
and gating ASIC RX on its state would prevent every response from reaching
the ESP32. Read-only info and receive-loss counters remain on UART0.

The ESP32 must establish safe outputs at boot, monitor and latch ASIC trip,
sequence power/reset, and restore safe outputs on host disconnect. Anvil
manages TPS546 output and board commands through that ESP32 firmware.
Programming the bridge leaves ASIC reset asserted, 5 V/TPS off and fan output
full; completion never enables ASIC power.

Bridge RX starts at boot and waits for idle high before a start bit. It samples
nine data bits, requires the stop/idle level, drops bit 8 and forwards raw bytes.
A 1024-word DMA ring separates real-time PIO reception from ESP UART forwarding;
overwrite/FIFO counters report receive loss. This is transport buffering, not
an independent power or temperature interlock.

The RP2040 watchdog restarts a stalled executor after three seconds. Restarting
the bridge does not change ESP32 board-control pins. No RP2040 watchdog action
substitutes for ESP32 trip and power management.
