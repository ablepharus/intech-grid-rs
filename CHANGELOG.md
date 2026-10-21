# Changelog

All notable changes to this project.

## 0.1.0 — 2026-10-21

First tagged release. Verified against an EN16 RevD and an EF44 RevD.

### Library (`intech-grid-rs`, imported as `intech_grid`)

- Packet framing, checksums and reply matching for the Grid serial protocol.
- Module table covering all 47 hardware ids the firmware defines, 44 with an
  element layout transcribed from `grid_module_*_ui_init`.
- Bus awareness: one USB connection carries a chain of modules, each with its
  own position. `scan_bus` enumerates them, the master is chosen
  deterministically, and packets are addressed at the chosen module.
- Static configuration: action-string parsing that rewrites single blocks and
  leaves every other byte intact, LED colours and curves, element settings,
  and whole-config snapshots with differential apply.
- Dialect handling for both firmware families, including re-framing stored
  action strings across the 1.5.2 wrapped/bare boundary on load.
- Live runtime control: knob modes (absolute, relative, relative-signed),
  LED colour, brightness and animation, and `eval` for reading live state back
  through the module's `DEBUGTEXT` output.
- Firmware capability gating by running version, with an explicit error rather
  than silently storing Lua the module cannot run.
- Firmware updates: UF2 parsing and validation including multi-architecture
  release bundles, bootloader volume discovery, and a gated flash.

### CLI (`gridctl`)

- `list`, `info`, `models`, with `--module X,Y` to target one module on a bus.
- `led`, `set`, `get`, `store`, `discard`, `reset` for configuration.
- `dump` / `load` for backup and restore.
- `live show|mode|anim|anim-clear` and `exec` for runtime control.
- `midi bind|show|unbind`, `rx-mode`, `value set` for software-driven values.
- `firmware status|validate|update`.
- `--json` output, `GRID_DEBUG=1` packet tracing.

### Notes

- Immediate scripts previously appeared inert. Firmware up to 1.5.1 drops any
  immediate script not wrapped in `<?lua ... ?>`, with no reply; 1.5.2 and
  newer require the bare form. Framing now follows the running version.
- `rx_mode` and `midirx_register` do not exist before firmware 1.5.2, so the
  MIDI binding commands require it and say so rather than failing silently.
