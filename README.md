# intech-grid-rs

A Rust library and CLI for Intech Studio **Grid** controllers over their USB
serial port — LED colours, encoder/fader ranges and modes, live runtime
control, whole-config dump/restore and firmware updates — without the Grid
editor. Every hardware id the firmware defines is recognised (47), and 44 have
a verified element layout.

> **Unofficial.** Not affiliated with, authorised by, or endorsed by Intech
> Studio. Built by reading the vendor's published firmware and protocol
> sources. Firmware flashing can brick hardware; see [API.md](API.md).

* `crates/intech-grid-rs` — the library (`intech_grid`).
* `crates/gridctl` — the `gridctl` command line tool.
* **[API.md](API.md) — full API and CLI reference with worked examples.**
* `vendor/` — upstream sources used as the protocol reference
  ([grid-fw](https://github.com/intechstudio/grid-fw),
  [grid-protocol](https://github.com/intechstudio/grid-protocol)). Reference
  only; nothing here is compiled.

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
gridctl list                      # which modules are connected
gridctl info                      # identity, firmware, element layout

gridctl led set 3 red             # encoder 3 red, stored on the module
gridctl led set all '#ff8800'      # every LED amber
gridctl led set 0-7 blue           # a range
gridctl led get                    # read back stored colours
gridctl led clear all              # back to the module default

gridctl set all max 100            # encoder range 0..100
gridctl get 0                      # settings of element 0
gridctl get 0 --raw                # ...plus the raw Lua it is stored as

gridctl dump -o backup.json        # back up the configuration
gridctl load backup.json           # restore it

gridctl led set 5 green --live      # light it now, change nothing stored

gridctl models                     # every Intech module, no hardware needed
gridctl models --supported

gridctl live show 0-3              # read mode/min/max/sensitivity back
gridctl live mode all relative     # absolute | relative | relative-signed
gridctl live anim 0 3 1            # LED animation: rate, type (0 0 = steady)
gridctl live anim-clear all

gridctl firmware status            # version + any bootloader volume
gridctl firmware validate fw.uf2   # check an image, writes nothing
gridctl firmware update fw.uf2 --yes   # flash (destructive, needs --yes)
```

Supported hardware: 47 hardware ids, 44 with a usable element layout —
PO16, BU16, PBF4, EN16, EF44, OCTV, ZONA, VSN1L, VSN1R, VSN2 and TEK2.
TEK1, PB44 and SOFT are identified but have no layout, because the firmware
has none for them either and logs `Unknown Module`. See
[API.md](API.md#supported-modules).

Colours accept a name (`red`, `green`, `blue`, `yellow`, `purple`, `white`,
`cyan`, `orange`, `pink`, `lila`, `teal`, `amber`, `off`), `#rrggbb`,
`rrggbb`, `#rgb` or `r,g,b`. Elements accept an index, a range (`0-7`), a list
(`0,2,5`) or `all`.

### Persistent vs live

Changes are **persistent by default**: the new config is written to the active
page and then committed to the module's non-volatile memory, so it survives a
power cycle.

* `--no-store` keeps the change in RAM only (commit later with `gridctl store`,
  throw it away with `gridctl discard`).
* `--live` bypasses stored configuration entirely and drives the LED right
  now. It is overwritten as soon as the element is touched, which is what you
  want for show control.

### Turning LEDs off

```bash
gridctl led off 4            # element 4 dark, stored on the module
gridctl led off all
```

This zeroes the colour in *every* event script the element has, not just the
one that reacts to turning it. An encoder also has a button event with its own
colour block, so setting only one leaves the LED lighting up again the moment
the element is pressed. A single-point colour curve derives the dim and mid
colours from the max, so an all-zero colour is dark at every value.

### Driving values and LED brightness from software

The firmware's own MIDI receive binding does both at once: an incoming message
moves the element's value *and* scales its LED, using
`glp(layer, gmaps(midi_value, min, max, 0, 255))`. So one mechanism covers
both "scale the LED with a MIDI value" and "assign a value from software".

```bash
# Let each element follow its own CC: element N listens on 20 + N.
gridctl midi bind all --channel 0 --command 176 --cc-base 20

# The module only acts on MIDI itself when handle-internal routing is on.
gridctl rx-mode 4 --persist

# Bindings install from the element's init script, so reload once.
gridctl reset

# Now assign values from software; the LED follows and the next physical
# turn continues from the assigned position instead of the old one.
gridctl value set 0 50 --percent
gridctl value set all 100 --percent
gridctl value set 3 64              # raw MIDI value 0-127

gridctl midi show all               # read the stored bindings back
gridctl midi unbind all
```

`value set` writes to the module's USB MIDI node (`/dev/snd/midiC*D0`), found
the same way as the control port. `--led-only` on `bind` scales the LED
without moving the value; `--value-only` does the reverse.

### A limitation worth knowing

Immediate scripts (the `IMMEDIATE` class, used by `gridctl exec` and the
`--live` flags) produce **no observable effect** on the EN16 here running
firmware 1.5.1: Lua `print` output never comes back, `gms` emits no MIDI, and
`page_next` does not change the active page. Everything in this tool that
matters therefore goes through stored configuration, which is verified, and
through MIDI for runtime changes. Treat `exec`, `--live` and
`value set --via-immediate` as unverified on this firmware.

### Speed and reliability

Every reply is matched against the request that asked for it: page, element
and event for a read, the echoed message id for a write. Anything else is
discarded and the request is re-sent.

This is not optional. The module tends to hold a finished reply until more
input arrives, so a client that accepts whichever packet shows up first reads
the *previous* request's answer - which looks much faster and silently shifts
every colour by one element. Re-sending is what releases the held reply, and
is always safe because every request here is idempotent.

Measured on an EN16 (firmware 1.5.1): about 0.3s per configuration request,
so roughly 2s to read six LED colours and about 20s for a full `dump` or
`load` of all 67 scripts. `load` only writes what actually differs.

Set `GRID_DEBUG=1` to trace every packet sent and received:

```bash
GRID_DEBUG=1 gridctl led get 0
```

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
dev.flash_led_value(3, 200, None)?;

// Read and edit stored settings.
dev.set_element_setting(0, 3, "ema", 100)?;
let (_event, action) = dev.element_action(0, 3)?;
assert_eq!(action.setting("ema"), Some(100));
```

### Using it from another project

Add a path dependency, for example in `~/orb/Cargo.toml`:

```toml
[workspace.dependencies]
intech-grid-rs = { path = "../intech-grid-rs/crates/intech-grid-rs" }
```

then `intech-grid-rs = { workspace = true }` in the crate that needs it. The
import path is `intech_grid`, not the package name:

```rust
use intech_grid::{Device, Rgb};
```

The library is sync and holds the serial port open; from async code, drive it
on a blocking task.

## How it works

Everything was transcribed from the open-source firmware and verified against
real hardware.

Grid modules expose a USB CDC serial port (`303a:8123`, shows up as
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
| `HEARTBEAT` | `0x010` | module identity: `HWCFG` byte, firmware version |
| `PAGEACTIVE` | `0x030` | which page is live |
| `CONFIG` | `0x060` | `FETCH` reads, `EXECUTE` writes one element/event script |
| `PAGESTORE` | `0x061` | commit the page to non-volatile memory |
| `PAGEDISCARD` / `PAGECLEAR` | `0x063` / `0x064` | revert / reset to defaults |
| `IMMEDIATE` | `0x085` | run Lua on the module now |

Addressing matters: the firmware's `grid_check_destination` only treats
`dx == dy == 127` as "this module", and the `CONFIG` class accepts nothing
else, so every packet is addressed that way rather than globally (`0,0`).
Configuration writes are also only accepted for the page that is currently
active, which is why `--page` defaults to it.

Configuration lives as one Lua "action string" per `(page, element, event)`,
split into blocks introduced by `--[[@id]]`. The firmware's own default for an
encoder event is:

```text
--[[@sec]]self:emo(0)self:ev0(50)self:emi(0)self:ema(127)self:ese(100)
--[[@sglc]]self:glc(-1,{{-1,-1,-1,1}})self:glp(-1,-1)
--[[@gms]]self:gms(-1,-1,-1,-1)
```

So a colour change rewrites exactly the `self:glc(...)` call inside the
`sglc` block and leaves every other byte alone — `-1` means "use the element
default". The colour argument is a curve of `{r,g,b,alpha}` points; with a
single point the firmware derives the dim and mid colours from it
(`/20` and `/2`). `glc`'s layer argument of `-1` resolves per event handler
(encoder → layer 2, button/fader → layer 1), per
`grid-fw/common/src/lua/simplecolor.lua`.

### Module table

Element and LED counts come from `grid_module_*_ui_init` in
`grid-fw/common/src/c/grid_module.c`:

| Module | Elements | LEDs |
|---|---|---|
| EN16 | 16 encoders + system | 16 |
| PBF4 | 8 potmeters/faders + 4 buttons + system | 12 |
| PO16 | 16 potmeters + system | 16 |
| BU16 | 16 buttons + system | 16 |

Other modules (EF44, OCTV, ZONA, TEK, VSN, PB44) are identified by name and
revision, but their element layouts have not been transcribed, so
element-level commands refuse to guess.
