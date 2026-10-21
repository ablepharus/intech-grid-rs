# intech-grid-rs / gridctl — API and CLI reference

> **Unofficial.** Not affiliated with, authorised by, or endorsed by Intech
> Studio. Derived from the vendor's published firmware and protocol sources.

A Rust library and CLI for configuring Intech Studio **Grid** controllers over
their USB serial port: LED colour and animation, knob behaviour, MIDI
bindings, whole-config backup/restore, and firmware updates.

Everything here was transcribed from the open-source firmware
([grid-fw](https://github.com/intechstudio/grid-fw)) and, where noted,
verified against real hardware.

- [Install and build](#install-and-build)
- [CLI reference](#cli-reference)
- [Library API](#library-api)
- [Supported modules](#supported-modules)
- [Firmware updates](#firmware-updates)
- [Verified vs unverified](#verified-vs-unverified)

---

## Install and build

The repo has no C toolchain of its own, so build inside a shell that provides
a linker. The `orb` dev shell works:

```bash
nix develop ~/orb --command cargo build --release
nix develop ~/orb --command cargo test
./target/release/gridctl --help
```

---

## CLI reference

Conventions used everywhere:

| Concept | Accepted forms |
|---|---|
| Element selector | `3`, `0-7`, `0,2,5`, `all` |
| Colour | `red`, `#ff8800`, `ff8800`, `#f80`, `255,136,0`, `off` |
| Page | `--page N`; defaults to the **active** page (the only page the firmware accepts writes for) |
| Persistence | persistent by default; `--no-store` keeps it in RAM; `gridctl store` commits later |
| Output | add `--json` to any command for machine-readable output |

### Discovery and identity

```bash
# Which modules are attached?
gridctl list
# /dev/ttyACM0  EN16 RevD  firmware 1.5.1  17 elements, 16 LEDs

# Full identity and element map of one module
gridctl info
# module      EN16 RevD
# hwcfg       0xc1
# firmware    1.5.1
# elements    17 (16 LEDs)
#   0-15    encoder
#   16      system

# Every module this tool knows — needs no hardware
gridctl models
gridctl models --supported        # only the ones with a usable layout
gridctl --json models | jq '.[] | select(.kind=="EN16")'
```

### Multiple modules on one bus

Grid modules chain physically and share a **single USB connection**. Only the
module plugged into the host is the *master*; every module on the chain
broadcasts its own heartbeat over that one serial port, identified by its
position. A port is therefore a bus, not a device.

```bash
gridctl list
# /dev/ttyACM0
#   EN16 RevD      at 127,127 firmware 1.5.1    17 elements, 16 LEDs  (master)
#   EF44 RevD      at 128,127 firmware 1.2.35    9 elements,  8 LEDs
#   target one with --module X,Y

gridctl info                       # the master, always deterministic
gridctl --module 128,127 info      # the downstream module
gridctl --module 128,127 led get   # its LEDs, not the master's
gridctl --module 128,127 dump -o ef44.json
```

`--module X,Y` works on every command. Without it the master is used. The
master sits at `127,127`, which the firmware also reads as "me"; a downstream
module's own position routes the packet along the chain.

This matters for correctness, not just convenience: a client that takes the
first heartbeat it sees gets a *random* identity on a chained bus, and then
writes land on the master while reporting another module's element layout.

### LED colour

```bash
gridctl led set 3 red             # one element, stored on the module
gridctl led set all '#ff8800'     # every LED
gridctl led set 0-7 blue          # a range
gridctl led set 4 red --alpha 0.4 # dimmed on the device

gridctl led get                   # read stored colours back
gridctl led get 0-3

gridctl led off 4                 # dark, stored
gridctl led off all
gridctl led clear all             # back to the module default colour

gridctl led set 5 green --live    # light it now, stored config untouched
```

`led off` zeroes the colour in **every** event script the element owns, not
just the one that reacts to turning it — an encoder also has a button event
with its own colour block, so setting only one leaves the LED lighting up
again as soon as it is pressed.

### LED animation

Maps to the firmware's `glf` (rate) and `gls` (type); `0 0` is steady.

```bash
gridctl live anim 0 3 1           # element 0: rate 3, type 1, stored
gridctl live anim all 0 0         # steady again
gridctl live anim 2 5 1 --live    # drive it now, nothing stored
gridctl live anim 2 5 1 --layer 1 # pick the LED layer explicitly
gridctl live anim-clear all       # remove the stored animation block
```

### Knob behaviour (absolute / relative)

```bash
gridctl live show                 # read mode/min/max/sensitivity back
gridctl live show 0-3
gridctl --json live show 0

gridctl live mode all absolute          # value is a position, clamped to min..max
gridctl live mode 0-3 relative          # re-centres to the middle on init
gridctl live mode 0 relative-signed     # signed delta, DAW-style endless
```

The three modes are the firmware's own, from `grid_ui_encoder.c`:

| Mode | Firmware value | Behaviour |
|---|---|---|
| `absolute` | 0 | Accumulates, clamped to `min..=max`. Nothing resets it. Default. |
| `relative` | 1 | Same accumulation, but re-centred to `((max+1)+min)/2` on event init. |
| `relative-signed` | 2 | Stores a signed delta in two's complement, clamped to `-range/2 ..= range/2-1`; initialises to `min`. |

Elements without a movement mode (buttons, system) are skipped and reported,
not silently ignored.

### Individual settings

```bash
gridctl get 0                     # settings of element 0
gridctl get 0 --raw               # ...plus the raw Lua it is stored as
gridctl set all max 100           # friendly names: min, max, mode, sensitivity, velocity
gridctl set 0 ema 127             # or a raw firmware short name
```

### Assigning a value from software

```bash
# One-time setup: let each element follow its own CC, then reload so the
# init scripts install the bindings.
gridctl midi bind all --channel 0 --command 176 --cc-base 20
gridctl rx-mode 4 --persist
gridctl reset

# Now assign values; the LED follows and the next physical turn continues
# from the assigned position.
gridctl value set 0 50 --percent
gridctl value set all 100 --percent
gridctl value set 3 64                 # raw MIDI value 0-127
```

### MIDI bindings

```bash
gridctl midi bind all --cc-base 20     # element N listens on CC 20+N
gridctl midi bind 0 --channel 0 --command 176 --cc 20
gridctl midi bind 0-3 --led-only       # scale the LED, leave the value alone
gridctl midi bind 0-3 --value-only     # move the value, leave the LED alone
gridctl midi show all
gridctl midi unbind all

gridctl rx-mode 4 --persist   # 1=forward-from-usb 2=handle-external 4=handle-internal
```

### Backup and restore

```bash
gridctl dump -o backup.json       # the active page
gridctl dump --page 0 --page 1 -o two-pages.json
gridctl load backup.json          # writes only what differs
gridctl load - < backup.json
```

### Pages and lifecycle

```bash
gridctl store                     # commit the active page to memory
gridctl discard                    # drop unsaved changes
gridctl clear-page                 # reset the page to firmware defaults
gridctl reset                      # reboot the module
gridctl exec 'ele[0]:glc(2,{{255,0,0,1}})'   # raw Lua on the module
gridctl exec "print('emo='..ele[0]:emo())"   # prints come back to you
```

### Firmware

```bash
gridctl firmware status           # current version + any bootloader volume
gridctl firmware validate fw.uf2  # check an image, writes nothing
gridctl firmware update fw.uf2    # dry run: shows what it would do
gridctl firmware update fw.uf2 --yes   # actually flash (destructive)
```

---

## Library API

Add it as a path dependency. Note the package name carries the `-rs` suffix
while the import path does not:

```toml
[dependencies]
intech-grid-rs = { path = "../intech-grid-rs/crates/intech-grid-rs" }
```

```rust
use intech_grid::{Device, Rgb};   // lib name is intech_grid
```

### Open a module and read its identity

```rust
use intech_grid::Device;

let mut dev = Device::open_single()?;           // or Device::open(Path::new("/dev/ttyACM0"))
println!("{} firmware {}", dev.info().module, dev.info().firmware);
let page = dev.active_page()?;
# Ok::<(), intech_grid::Error>(())
```

### LED colour, persistent and live

```rust
use intech_grid::{Device, Rgb};

let mut dev = Device::open_single()?;
let page = dev.active_page()?;

// Persistent: edit the stored config, then commit it.
dev.set_led_color(page, 3, "#ff0000".parse()?, 1.0)?;
dev.store_page()?;

// Every event of the element, so it stays dark when pressed too.
dev.set_led_off(page, 3)?;

// Live: light it now, stored config untouched.
dev.flash_led(3, Rgb::new(0, 255, 0), None)?;
dev.flash_led_value(3, 200, None)?;
# Ok::<(), intech_grid::Error>(())
```

### Knob behaviour

```rust
use intech_grid::{Device, KnobMode};

let mut dev = Device::open_single()?;
let page = dev.active_page()?;

// Read before writing: None means "element default".
let s = dev.element_settings(page, 0)?;
println!("mode={:?} min={:?} max={:?}", s.mode, s.min, s.max);

dev.set_knob_mode(page, 0, KnobMode::RelativeSigned)?;
dev.set_element_setting(page, 0, "ema", 100)?;
dev.store_page()?;
# Ok::<(), intech_grid::Error>(())
```

### LED animation

```rust
use intech_grid::{Device, LedAnimation};

let mut dev = Device::open_single()?;
let page = dev.active_page()?;

dev.set_led_animation(page, 0, LedAnimation::new(3, 1), None)?;
dev.store_page()?;

dev.set_led_animation(page, 0, LedAnimation::STEADY, None)?;   // stop animating
dev.flash_led_animation(0, LedAnimation::new(5, 1), None)?;    // right now
# Ok::<(), intech_grid::Error>(())
```

### The model table (no hardware needed)

```rust
use intech_grid::module::{Module, MODULES};

// Identify a module from the hwcfg byte in its heartbeat.
let m = Module::from_hwcfg(0xc1)?;
assert_eq!(m.name(), "EN16 RevD");
assert_eq!(m.element_count(), 17);          // 16 encoders + system
assert_eq!(m.led_elements().count(), 16);

// Or walk everything this crate knows.
let supported = MODULES.iter().filter(|m| m.layout_known()).count();
assert_eq!(MODULES.len(), 47);
assert_eq!(supported, 44);
# Ok::<(), intech_grid::Error>(())
```

### Backup and restore

```rust
use intech_grid::{snapshot, Device, Snapshot};

let mut dev = Device::open_single()?;
let page = dev.active_page()?;

let snap = snapshot::dump(&mut dev, &[page])?;
std::fs::write("backup.json", snap.to_json()?)?;

let restored = Snapshot::from_json(&std::fs::read_to_string("backup.json")?)?;
let report = snapshot::apply(&mut dev, &restored)?;   // writes only differences
println!("{} written, {} already matched", report.written.len(), report.unchanged);
# Ok::<(), intech_grid::Error>(())
```

### Firmware images

```rust
use intech_grid::firmware::{self, Uf2Image};
use std::path::Path;

// Validation is pure and needs no device.
let img = Uf2Image::open(Path::new("grid.uf2"))?;
println!("{} blocks, family 0x{:08x}", img.blocks, img.family_id);

// Flashing requires the module to be in its bootloader.
for vol in firmware::find_bootloader_volumes()? {
    let written = firmware::flash(&vol, &img)?;   // destructive
    println!("wrote {written} bytes to {}", vol.mount_point.display());
}
# Ok::<(), intech_grid::Error>(())
```

---

## Supported modules

47 hardware ids, 44 with a usable element layout. Run `gridctl models` for the
full table with revisions and variants.

| Module | Elements | LEDs | Controls |
|---|---|---|---|
| PO16 | 17 | 16 | 16 potentiometers |
| BU16 | 17 | 16 | 16 buttons |
| PBF4 | 13 | 12 | 8 potentiometers/faders, 4 buttons |
| EN16 | 17 | 16 | 16 rotary encoders |
| EF44 | 9 | 8 | 4 rotary encoders, 4 faders |
| OCTV | 22 | 21 | 8 rotary encoders, 13 buttons |
| ZONA | 2 | 81 | touch surface, 81-LED matrix |
| VSN1L / VSN1R | 15 | 18 | 8+4 buttons, 1 endless pot, 1 LCD |
| VSN2 | 19 | 18 | 12+4 buttons, 2 LCDs |
| TEK2 | 11 | 18 | 8 buttons, 2 endless pots |
| TEK1, PB44, SOFT | — | — | identified only; see below |

Every layout is transcribed from the matching `grid_module_*_ui_init` in
`grid_module.c`, and the hwcfg→layout mapping from the
`grid_hwcfg_module_is_*` predicates in `grid_sys.c` plus the dispatch chain in
`esp32s3/main/grid_esp32s3.c`.

**TEK1, PB44 and SOFT have no layout because the firmware has none either** —
no `*_ui_init` exists and no dispatch branch matches, so the module itself
logs `UI Init failed: Unknown Module`. This crate reports that instead of
guessing.

ZONA's 81 LEDs hang off a single touch element with no per-element value
event, so they are not addressable through the colour API. `led_elements()`
returns empty for it, deliberately.

---

## Firmware updates

Grid modules on ESP32-S3 ship the **TinyUF2** bootloader, so updating is a
file copy rather than a protocol exchange.

1. **Enter the bootloader**: unplug every module, then **hold the utility
   button on the side** of the one you are updating and keep holding it while
   you plug in the USB cable. The LEDs go red, then green once the bootloader
   has enumerated, and the module appears as a USB drive named `GRID-S3`
   (or `GRID23`) instead of a serial port.
   There is no software command for this — `RESET` (0x013) restarts the
   application and `NVMERASE` (0x062) erases config; neither enters the
   bootloader, and no bootloader-entry path exists anywhere in the firmware
   sources.

   Two vendor constraints apply: update modules **one at a time**, with only
   that module connected, and keep **every module on the same firmware
   version** — a chain running mixed versions is unsupported. A chained
   downstream module therefore cannot be updated in place; connect it to USB
   on its own.
2. **Validate** the image: `gridctl firmware validate grid.uf2`. This checks
   the UF2 magic words, 512-byte block alignment, the declared block count
   and the ESP32-S3 family id `0xc47e5767`, so a wrong file cannot reach the
   volume. A raw `.bin` is rejected with a pointer to esptool.
3. **Flash**: `gridctl firmware update grid.uf2 --yes`. Without `--yes` it
   prints what it would do and exits. The volume disappears as the bootloader
   reboots, which the tool treats as success rather than an I/O error.

**Recovery.** A module with a damaged application can still be flashed over
serial with `esptool.py`, as the vendor's `burn_test_firmware.sh` does:
bootloader at `0x0`, partition table at `0x8000`, OTA data at `0xe000`, the
application at `0x10000`, TinyUF2 at `0x410000`.

---

## Verified vs unverified

Honesty about evidence, because some of this cannot be proven without hardware
attached.

**Verified on hardware** (EN16 RevD fw 1.5.1, EF44 RevD fw 1.2.35): module
identification; config read/write; LED colour set/read/off, persisted and
surviving a reboot; element settings round-trip; MIDI binding storage; dump
and load in both directions.

**Verified without hardware**: the full model table against the firmware
sources; UF2 validation against the three real vendor images shipped in
`vendor/grid-fw/esp32s3/bootloader/`; `gridctl models`; `firmware status` and
`validate`.

**Not verified**: the actual firmware flash (needs a module in bootloader
mode and consent), and the live runtime paths below.

### Firmware feature matrix

Grid firmware gained capabilities over time, and a module **silently ignores
Lua it does not know** — a missing global raises an error inside the script,
which then stores or runs but does nothing. The library checks the running
version first and refuses with an explicit error instead.

| Capability | Needs | ≤1.2.x | 1.5.0 – 1.5.1 | ≥1.5.2 |
|---|---|---|---|---|
| Static config (CONFIG 0x060), LED colour, settings, dump/load | any | yes | yes | yes |
| LED animation (`glf` / `gls`) | any | yes | yes | yes |
| Immediate scripts (`IMMEDIATE` 0x085) | 1.5.0 | **no handler at all** | yes, **wrapper required** | yes, **bare** |
| `rx_mode(..)` | 1.5.2 | no | no | yes |
| `midirx_register(..)` / `self:gmrr(..)` | 1.5.2 | no | no | yes |
| Stored action strings | — | wrapped | wrapped | bare |

Both modules on this bench now run **1.5.6**, so every row above is available;
the table is kept because the library still supports older firmware and gates
each call on the running version.

Verified by diffing the firmware tags in `vendor/grid-fw`: `grid_decode.c` at
`v1.2.39` contains no immediate handler, and `GRID_LUA_FNC_G_RX_MODE_short` /
`GRID_LUA_FNC_G_MIDIRX_REGISTER_short` first appear at `v1.5.2`. The stored
action convention is visible in the firmware's own defaults:
`GRID_ACTIONSTRING_ENCODER_INIT` is `"<?lua --[[@cb]] --[[Encoder Init]] ?>"`
at `v1.5.1` and `"--[[@cb]]--[[Encoder Init]]"` at `v1.5.6`.

A capability missing on a module is therefore a property of its **firmware
version, not of the model**: an EF44 on 1.2.35 cannot run immediate scripts at
all, and the same EF44 on 1.5.6 can. Updating the firmware is the fix, and
`Device::supports` reports the current answer rather than a fixed one.

### Root cause: why immediate scripts appeared dead

Earlier bench runs saw immediate scripts do nothing on an EN16 at 1.5.1 —
`print` produced no output, `gms` emitted no MIDI, `page_next` did not change
the page. The cause is in the firmware, and it is not a mystery any more.

`grid_decode_immediate_to_ui` at `v1.5.1` guards the script:

```c
char* script = &chunk[GRID_CLASS_IMMEDIATE_ACTIONSTRING_offset];
if (strncmp(script, "<?lua ", 6) != 0) {
  return 1;                                    /* dropped, no reply */
}
if (strncmp(&script[length - 3], " ?>", 3) != 0) {
  return 1;                                    /* dropped, no reply */
}
```

**Firmware up to 1.5.1 accepts only `<?lua … ?>`-wrapped immediate scripts.**
From 1.5.2 the check is gone and the handler runs the bare string, where a
wrapper would instead be a Lua syntax error. The packet was well formed — the
class, offsets and checksum all matched, which is why there was no NAK — it
was simply discarded at that guard.

`Device::immediate` now frames the script to match the running firmware, so
`--live` and `exec` are correct on both sides of 1.5.2, and on firmware older
than 1.5.0 they are refused outright because no handler exists.

Two consequences worth knowing:

- The `midi bind` / `rx-mode` path, and therefore `value set` over MIDI, needs
  **1.5.2 or newer**. On an older module those commands now fail with a clear
  message rather than storing Lua that cannot run.
- A script that *is* delivered but raises a Lua error leaves no trace on the
  wire either: in 1.5.6 `grid_lua_dostring_begin` takes the Lua semaphore and
  returns early on both `luaL_loadstring` and `lua_pcall` failure **without
  releasing it**, so one bad snippet can wedge further Lua execution until the
  module reboots. Prefer stored configuration for anything that must be
  reliable, and power-cycle if live scripting stops responding.

### Verified on hardware

Confirmed on a real two-module bus (EN16 RevD fw 1.5.1 as master at 127,127,
EF44 RevD fw 1.2.35 downstream at 128,127):

- **Live scripting works, and the wrapper is the reason it did not.** Same
  packet, same session, only the framing differs:

  ```
  [WRAPPED   print('ULWMARK')]  marker=true   -> \x02 020 e ULWMARK \x03
  [UNWRAPPED print('ULWMARK')]  marker=false  -> silently dropped
  ```

  Lua output comes back as a `DEBUGTEXT` (0x020) packet, which is why
  `gridctl exec` can now print what a script produced.
- **Live knob mode**, with an independent readback each step:
  `emo=0` -> set relative -> `now=1` -> re-read `emo=1` -> restore `0`.
- **Config round-trip, both dialects**: EN16 dump/load reported
  `0 script(s) written, 67 already matched` and a re-dump was byte-identical;
  EF44 reported `0 written, 31 matched`.
- **Per-module routing**: the EF44 reads back 8 LEDs all `#000000` while the
  master reads 16 LEDs with different colours, over the same port.
- **Firmware status** reports the running version and the absence of a
  bootloader volume.

Still not executed: the firmware **flash** itself, which needs a module in
bootloader mode and explicit consent. Validation, volume discovery and the
`--yes` gate are verified; the write is not.
