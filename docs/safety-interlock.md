# Bridge safety interlock

## Purpose and scope

The Bridge safety interlock keeps the board-control outputs owned by the RP2040
in a defined fail-closed state. It protects against:

- loss of ESP control traffic;
- an asserted active-high ASIC trip input;
- invalid power, reset, or fan command ordering; and
- a stalled Bridge control task.

The safe output request is always:

- 5 V disabled;
- ASIC reset asserted (`RST_N` driven low); and
- fan commanded to 100 percent.

This interlock reports effective RP2040 commands. It does not measure the
physical 5 V rail, VCORE, PGOOD, temperature, or actual fan speed. The current
hardware also gives the Bridge no independent VCORE cutoff. A system requiring
those protections must provide and verify them outside this firmware.

## Module ownership

The implementation is intentionally divided by responsibility:

| Module | Responsibility |
|---|---|
| `src/safety.rs` | Pure safety state machine, transition validation, fault latch, status encoding, and host tests |
| `src/control/mod.rs` | Samples the trip input, advances the state machine, applies effective GPIO/fan outputs, and feeds the RP2040 watchdog |
| `src/control/system.rs` | Exposes safety commands and status on system page `0x00` |
| `src/control/gpio.rs` | Routes 5 V and reset writes through the safety policy |
| `src/control/fan.rs` | Routes fan writes through the safety policy and services safety during tach measurement |
| `src/safety_timing.rs` | Timing constants and small calculations only; it does not own supervision or safety state |
| `src/main.rs` | Establishes safe boot pin requests and starts the hardware watchdog |

ASIC RX forwarding follows the effective safety outputs. It is enabled only
when 5 V is enabled and reset is released. Returning safe aborts DMA, drains
the PIO FIFO, and resets RX framing before another controlled session.

## State machine

| State | Meaning | Permitted exit |
|---|---|---|
| `SAFE_OFF` | Safe outputs, no active lease | `ARM` with trip input clear |
| `CONTROLLED` | Host may request ordered output changes while the lease remains valid | `DISARM`, lease expiry, or ASIC trip |
| `FAULT_LATCHED` | Safe outputs override all requested outputs | `CLEAR_FAULT` after the trip input is clear |

`ARM` always begins from the safe output request. It does not energize 5 V or
release reset by itself.

The following requests move the system toward safety and remain available
without an active lease:

- disable 5 V;
- assert ASIC reset; and
- command the fan to 100 percent.

Requests that can move away from safety require `CONTROLLED` state and a live
lease. The policy additionally enforces:

1. fan at 100 percent and reset asserted before enabling 5 V;
2. 5 V enabled before releasing ASIC reset; and
3. 5 V disabled before lowering the fan below 100 percent.

Disabling 5 V also asserts reset and restores the full-fan request.

## Host integration

A host should use this sequence:

1. Query `GET_INFO` and require a compatible protocol.
2. Query `GET_SAFETY_STATUS`.
3. Require `SAFE_OFF`, no fault, trip clear, 5 V off, reset asserted, and fan
   at 100 percent.
4. Send `ARM`.
5. Confirm `CONTROLLED`, a nonzero lease, no fault, and trip clear.
6. Keep the fan at 100 percent.
7. Enable 5 V.
8. Release reset only after any host-owned physical rail checks pass.
9. Send `HEARTBEAT` well inside the two-second lease while controlled.
10. For shutdown, assert reset, disable 5 V, command full fan, and send
    `DISARM`.
11. Confirm coherent `SAFE_OFF` status.

The safety commands are:

| System command | Value | Use |
|---|---:|---|
| `GET_SAFETY_STATUS` | `0x10` | Read the current state, fault, lease, evidence, and effective commands |
| `ARM` | `0x11` | Enter `CONTROLLED` from safe outputs |
| `HEARTBEAT` | `0x12` | Renew a live controlled lease |
| `CLEAR_FAULT` | `0x13` | Clear a latched fault only after trip is low; returns to `SAFE_OFF` |
| `DISARM` | `0x14` | Request safe outputs and end a healthy controlled session |

A host must treat command error `0x12` as a denied transition and `0x13` as a
latched or active safety fault. Retrying a heartbeat is not fault recovery.

## Timing and failure behavior

The fixed production timing is:

| Mechanism | Bound |
|---|---:|
| Output lease | 2000 ms |
| RP2040 hardware watchdog | 3000 ms |
| Control UART response write | 100 ms |
| Safety service interval during the 500 ms tach measurement | at most 10 ms |
| Partial control packet wait | approximately 4 ms |

Only the control task feeds the hardware watchdog, and it does so after it has
sampled the trip input, advanced the lease/fault policy, and applied effective
outputs.

Failure responses are:

| Condition | Bridge response |
|---|---|
| Lease expires | Latch `LEASE_EXPIRED` and request safe outputs |
| ASIC trip asserts | Latch `ASIC_TRIP` and request safe outputs |
| Host requests an invalid sequence | Reject it without applying the unsafe transition |
| Host stops sending complete packets | Discard the partial packet; lease timing continues |
| UART response cannot complete | Abandon the response after 100 ms; safety service continues |
| Control task stalls | RP2040 watchdog resets the firmware into safe boot initialization |

The trip input is sampled by the same control task that handles commands. It is
not an independent hardware trip monitor.

## Status and capability limits

The status payload distinguishes:

- configured policy stage;
- current state and latched fault;
- runtime and production verdicts;
- implemented capability bits;
- currently observed evidence bits;
- lease time remaining;
- effective output commands; and
- sampled trip input.

Current capability bits are `0x000f`: 5 V control, ASIC reset control, full-fan
command, and trip sampling. The Bridge intentionally does not claim:

- independent VCORE cutoff;
- autonomous fan-tach interlock; or
- trip monitoring independent of the command task.

Because those capabilities are absent, a healthy current build reports the
production capability-gap verdict. This is truthful protocol evidence, not a
runtime fault. Do not change the capability or verdict bits without matching
hardware integration and tests.

## Verification

Run the host-visible state-machine and timing tests with:

```sh
cargo test --lib --target x86_64-unknown-linux-gnu
```

The tests cover safe boot semantics, ordered transitions, lease expiry,
trip-latching, fault clearing, encoded status coherence, timing boundaries, and
fan tach conversion. The RP2040 release build must also compile successfully so
the pure policy remains correctly wired into the hardware control task.
