# intech-grid-rs

A Rust library and CLI for **Intech Studio Grid** controllers over their USB
serial port — LED colours, element settings, live runtime control, whole-config
dump and restore, and firmware updates — without the Grid editor.

Every hardware id the firmware defines is recognised (**47**), and **44** have
a verified element layout transcribed from the firmware's own `*_ui_init`.

> **Unofficial.** Not affiliated with, authorised by, or endorsed by Intech
> Studio. Built by reading the vendor's published firmware and protocol
> sources. Firmware flashing can brick hardware — read
> [Firmware updates](#firmware-updates) before using it.

| | |
|---|---|
| `crates/intech-grid-rs` | the library; import path is `intech_grid` |
| `crates/gridctl` | the `gridctl` command line tool |
| [**API.md**](API.md) | full API and CLI reference with worked examples |
| `vendor/` | upstream sources used as the protocol reference ([grid-fw](https://github.com/intechstudio/grid-fw), [grid-protocol](https://github.com/intechstudio/grid-protocol)). Reference only; nothing here is compiled. |

## Build

This repo has no `cc` of its own on NixOS, so build inside a shell that
provides a C linker. The `orb` dev shell works:

```bash
nix develop ~/orb --command cargo build --release
nix develop ~/orb --command cargo test
```

The binary lands in `target/release/gridctl`.

## Quick start

```bash
gridctl list                       # every module on every connection
gridctl info                       # identity, firmware, element layout

gridctl led set 3 red              # encoder 3 red, stored on the module
gridctl led set all '#ff8800'      # every LED amber
gridctl led get                    # read back stored colours
gridctl led off 4                  # element 4 dark
gridctl led clear all              # back to the module default

gridctl set all max 100            # encoder range 0..100
gridctl get 0 --raw                # settings of element 0, plus its raw Lua

gridctl dump -o backup.json        # back up the configuration
gridctl load backup.json           # restore it, writing only what differs

gridctl live show 0-3              # read mode/min/max/sensitivity back
gridctl live mode all relative     # absolute | relative | relative-signed
gridctl live anim 0 3 1            # LED animation: rate, type (0 0 = steady)
gridctl exec "print(gec())"        # run Lua on the module, see its output

gridctl models                     # every Intech module, no hardware needed
gridctl firmware status            # version + any bootloader volume
```

Colours accept a name (`red`, `green`, `blue`, `yellow`, `purple`, `white`,
`cyan`, `orange`, `pink`, `lila`, `teal`, `amber`, `off`), `#rrggbb`,
`rrggbb`, `#rgb` or `r,g,b`. Elements accept an index, a range (`0-7`), a list
(`0,2,5`) or `all`.

## A connection is a bus, not a device

Grid modules chain physically and share **one** USB connection. Only the
module plugged into the host is the *master*; every module on the chain
broadcasts its own heartbeat over that port, identified by its position.

```bash
gridctl list
# /dev/ttyACM0
#   EN16 RevD      at 127,127 firmware 1.5.6    17 elements, 16 LEDs  (master)
#   EF44 RevD      at 128,127 firmware 1.5.6     9 elements,  8 LEDs
#   target one with --module X,Y

gridctl info                       # the master, always deterministic
gridctl --module 128,127 led get   # the downstream module's LEDs
```

`--module X,Y` works on every command; without it the master is used. This is
a correctness matter, not a convenience: a client that takes the first
heartbeat it sees gets a *random* identity on a chained bus, and then writes
land on the master while it reports another module's layout.

## Persistent, stored-but-uncommitted, and live

Changes are **persistent by default**: written to the active page and then
committed to non-volatile memory, so they survive a power cycle.

* `--no-store` keeps the change in RAM only — commit later with
  `gridctl store`, discard it with `gridctl discard`.
* `--live` bypasses stored configuration entirely and drives the LED right
  now. It is overwritten as soon as the element is touched, which is what you
  want for show control.

### Turning LEDs off

`gridctl led off` zeroes the colour in *every* event script the element has,
not just the one that reacts to turning it. An encoder also has a button event
with its own colour block, so setting only one leaves the LED lighting up again
the moment the element is pressed. A single-point colour curve derives the dim
and mid colours from the max, so an all-zero colour is dark at every value.

## Firmware matters more than the model

Grid firmware gained capabilities over time, and a module **silently ignores
Lua it does not know**. The library checks the running version and refuses with
an explicit error instead of storing a script that cannot work.

| Capability | Needs | ≤1.2.x | 1.5.0–1.5.1 | ≥1.5.2 |
|---|---|---|---|---|
| Static config, LED colour, settings, dump/load | any | yes | yes | yes |
| LED animation (`glf` / `gls`) | any | yes | yes | yes |
| Immediate scripts (`gridctl exec`, `--live`) | 1.5.0 | no handler | yes, **wrapper required** | yes, **bare** |
| `rx_mode`, `midirx_register` (`midi bind`) | 1.5.2 | no | no | yes |
| Stored action-string form | — | wrapped | wrapped | bare |

Two consequences:

* **A missing capability is a property of the firmware, not the model.** An
  EF44 on 1.2.35 cannot run immediate scripts at all; the same EF44 on 1.5.6
  can. Updating the firmware is the fix.
* **`dump` / `load` across the 1.5.2 boundary is handled for you.** Stored
  action strings are wrapped in `<?lua ... ?>` up to 1.5.1 and bare from 1.5.2,
  so `load` re-frames them for the target rather than copying verbatim.

Immediate scripts once appeared completely dead, which earlier versions of this
document reported as an unexplained limitation. The cause was this framing:
firmware ≤1.5.1 drops any immediate script that is not wrapped, with no reply
at all. It is fixed and verified on hardware in both directions.

## Driving values and LED brightness from software

The firmware's MIDI receive binding does both at once: an incoming message
moves the element's value *and* scales its LED, via
`glp(layer, gmaps(midi_value, min, max, 0, 255))`. **Needs firmware 1.5.2 or
newer.**

```bash
gridctl midi bind all --channel 0 --command 176 --cc-base 20
gridctl rx-mode 4 --persist     # the module only acts on MIDI with this on
gridctl reset                   # bindings install from init, so reload once

gridctl value set 0 50 --percent
gridctl value set 3 64          # raw MIDI value 0-127
gridctl midi show all           # read the stored bindings back
```

`value set` writes to the module's USB MIDI node (`/dev/snd/midiC*D0`).
`--led-only` on `bind` scales the LED without moving the value; `--value-only`
does the reverse.

## Firmware updates

Updating is a file copy onto the TinyUF2 bootloader volume, not a protocol
exchange. **This overwrites the module's firmware and cannot be undone.**

Entering the bootloader is physical and there is **no software command** for it
— nothing in the firmware sources provides one:

1. Unplug every module, so only the one being updated is connected.
2. **Hold the utility button on the side** of the module and keep holding it
   while plugging in the USB cable. The LEDs go red, then green.
3. It appears as a USB drive named `GRID-S3` instead of a serial port. On
   systems without automounting it may show up as an unmounted block device —
   `udisksctl mount -b /dev/sdX`.

```bash
gridctl firmware status                  # running version + bootloader volume
gridctl firmware validate grid.uf2       # checks the image, writes nothing
gridctl firmware update grid.uf2 --yes   # flash; refuses without --yes
```

Validation checks the UF2 magic words, block alignment, declared block counts
and the ESP32-S3 family id `0xc47e5767`, so a wrong image cannot reach the
volume. Multi-architecture release bundles are understood: the vendor ships one
file concatenating per-chip segments, and the ESP32-S3 segment is selected from
it. A raw `.bin` is rejected with a pointer to esptool.

Back the configuration up first with `gridctl dump`. The vendor requires every
module on a chain to run the **same** firmware version, and because the
bootloader only appears for the module attached to USB, a downstream module
must be connected on its own to be updated.

Images come from
[grid-fw releases](https://github.com/intechstudio/grid-fw/releases)
(`grid_release.zip`); see the
[vendor update guide](https://docs.intech.studio/guides/troubleshooting/firmware-update/).

## Library

```rust
use intech_grid::{Device, Rgb};

let mut dev = Device::open_single()?;
println!("{} firmware {}", dev.info().module, dev.info().firmware);

// Persistent: edit the stored config, then commit it.
dev.set_led_color(0, 3, "#ff0000".parse()?, 1.0)?;
dev.store_page()?;

// Live: light it now, stored config untouched.
dev.flash_led(3, Rgb::new(0, 255, 0), None)?;

// Read live runtime state back; Lua output returns as a DEBUGTEXT packet.
let mode = dev.eval("ele[0]:emo()")?;
```

### Using it from another project

```toml
[dependencies]
intech-grid-rs = { path = "../intech-grid-rs/crates/intech-grid-rs" }
```

The import path is `intech_grid`, not the package name:

```rust
use intech_grid::{Device, Rgb};
```

The library is sync and holds the serial port open; from async code, drive it
on a blocking task.

## How it works

Everything was transcribed from the open-source firmware and verified against
real hardware.

Grid modules expose a USB CDC serial port (`303a:8123`, as
`/dev/serial/by-id/usb-Intech_Studio_Grid_*`). Packets are ASCII:

```text
SOH BRC LEN(4) ID(2) SESSION(2) SX(2) SY(2) DX(2) DY(2) ROT(1) PORTROT(1) MSGAGE(2) EOB
    STX <class:3> <instr:1> <payload...> ETX     (one or more bodies)
EOT <checksum:2> LF
```

`LEN` is the total length minus the trailing 3 bytes, and the checksum is an
XOR over every byte before those same 3. The classes this tool uses:

| Class | Code | Use |
|---|---|---|
| `HEARTBEAT` | `0x010` | module identity: `HWCFG` byte, firmware version, type |
| `DEBUGTEXT` | `0x020` | Lua `print` output — the only live readback channel |
| `PAGEACTIVE` | `0x030` | which page is live |
| `CONFIG` | `0x060` | `FETCH` reads, `EXECUTE` writes one element/event script |
| `PAGESTORE` | `0x061` | commit the page to non-volatile memory |
| `PAGEDISCARD` / `PAGECLEAR` | `0x063` / `0x064` | revert / reset to defaults |
| `IMMEDIATE` | `0x085` | run Lua on the module now |

**Addressing.** A module reads `dx == dy == 127` as "me" and forwards anything
else along the chain, and the editor encodes positions as `relative + 127`.
Packets are therefore addressed at the target module's own position, which is
what makes `--module` work. Configuration writes are only accepted for the
page that is currently active, which is why `--page` defaults to it.

**Configuration** lives as one Lua "action string" per `(page, element,
event)`, split into blocks introduced by `--[[@id]]`. The firmware's own
default for an encoder event is:

```text
--[[@sec]]self:emo(0)self:ev0(50)self:emi(0)self:ema(127)self:ese(100)
--[[@sglc]]self:glc(-1,{{-1,-1,-1,1}})self:glp(-1,-1)
--[[@gms]]self:gms(-1,-1,-1,-1)
```

A colour change rewrites exactly the `self:glc(...)` call inside the `sglc`
block and leaves every other byte alone — `-1` means "use the element
default". The colour argument is a curve of `{r,g,b,alpha}` points; with a
single point the firmware derives the dim and mid colours from it (`/20` and
`/2`). `glc`'s layer argument of `-1` resolves per event handler (encoder →
layer 2, button/fader → layer 1), per
`grid-fw/common/src/lua/simplecolor.lua`.

### Supported modules

Element and LED counts come from `grid_module_*_ui_init` in
`grid-fw/common/src/c/grid_module.c`. Element counts include the module's
system element.

| Family | Elements | LEDs | Controls |
|---|---|---|---|
| PO16 | 17 | 16 | 16 potentiometers |
| BU16 | 17 | 16 | 16 buttons |
| EN16 | 17 | 16 | 16 rotary encoders |
| PBF4 | 13 | 12 | 8 potentiometers/faders, 4 buttons |
| EF44 | 9 | 8 | 4 rotary encoders, 4 faders |
| OCTV | 22 | 21 | 8 rotary encoders, 13 buttons |
| ZONA | 2 | 81 | touch surface with an 81-LED matrix |
| VSN1L / VSN1R | 15 | 18 | 8+4 buttons, 1 endless potentiometer, 1 LCD |
| VSN2 | 19 | 18 | 12+4 buttons, 2 LCDs |
| TEK2 | 11 | 18 | 8 buttons, 2 endless potentiometers |

All revisions and variants of these resolve, including `ND` and `CD` — 44 ids
in total. `gridctl models` lists every one.

`TEK1`, `PB44` and `SOFT` are identified by name but have no layout, because
the firmware has none for them either: no `*_ui_init` exists, no dispatch
branch matches, and it logs `UI Init failed: Unknown Module`. Element-level
commands refuse rather than guess.

### Speed and reliability

Every reply is matched against the request that asked for it: page, element
and event for a read, the echoed message id for a write. Anything else is
discarded and the request re-sent.

This is not optional. The module tends to hold a finished reply until more
input arrives, so a client that accepts whichever packet shows up first reads
the *previous* request's answer — which looks much faster and silently shifts
every colour by one element. Re-sending is what releases the held reply, and
is always safe because every request here is idempotent.

Measured on an EN16: a full `dump` of all 67 scripts takes about 1.2s. `load`
writes only what actually differs, and reports the count.

Set `GRID_DEBUG=1` to trace every packet sent and received:

```bash
GRID_DEBUG=1 gridctl led get 0
```

## Verification status

Verified on hardware (EN16 RevD and EF44 RevD, both on firmware 1.5.6,
chained and individually): identity and bus enumeration, LED colours,
element settings, `dump`/`load` round-trips byte-identical on both firmware
dialects, live knob modes with independent readback, live LED colour,
brightness and animation, per-module addressing, firmware `status`,
`validate`, and a full `update` on both modules.

Covered by tests but not exercised on hardware: the 1.2.x LED dialect beyond
the round-trip above, and restoring a backup *across* the 1.5.2 boundary.

`cargo test` is 88 tests; `cargo clippy --all-targets` and `cargo doc` are
warning-free.

## License

MIT — see [LICENSE](LICENSE).

The vendored sources under `vendor/` belong to Intech Studio and keep their own
licences; they are reference material and are not compiled into this project.
