//! `gridctl` - configure Intech Studio Grid controllers from the command line.

mod selector;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use orb_intech_grid::{
    firmware, live::KnobMode, snapshot, Device, ElementKind, LedAnimation, Rgb,
};
use selector::Selector;

#[derive(Parser)]
#[command(
    name = "gridctl",
    about = "Configure Intech Studio Grid controllers (EN16, PBF4, PO16, BU16)",
    version,
    after_help = "Colours accept a name (red, green, blue, yellow, purple, white, cyan, orange, pink, lila, teal, amber, off), \
#rrggbb, rrggbb, #rgb or r,g,b.\n\
Elements accept an index (3), a range (0-7), a list (0,2,5) or 'all'."
)]
struct Cli {
    /// Serial port of the module. Defaults to the only connected Grid.
    #[arg(long, short, global = true)]
    port: Option<PathBuf>,

    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List connected Grid modules.
    List,
    /// Show module identity, firmware and element layout.
    Info,
    /// LED colour commands.
    Led {
        #[command(subcommand)]
        command: LedCommand,
    },
    /// Show an element's stored settings.
    Get {
        /// Element index.
        element: u8,
        /// Page to read. Defaults to the active page.
        #[arg(long)]
        page: Option<u8>,
        /// Print the raw Lua action strings too.
        #[arg(long)]
        raw: bool,
    },
    /// Change one element setting, e.g. `gridctl set 0 max 100`.
    Set {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// Setting name: min, max, mode, sensitivity, velocity, or a raw
        /// firmware name such as `ema`.
        name: String,
        /// New value.
        value: i64,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Dump the configuration to JSON.
    Dump {
        /// Pages to dump. Defaults to the active page.
        #[arg(long)]
        page: Vec<u8>,
        /// Write to a file instead of stdout.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Restore a configuration previously written by `dump`.
    Load {
        /// Snapshot file, or `-` for stdin.
        file: PathBuf,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Assign element values from software.
    Value {
        #[command(subcommand)]
        command: ValueCommand,
    },
    /// Bind elements to incoming MIDI.
    Midi {
        #[command(subcommand)]
        command: MidiCommand,
    },
    /// Show or set how incoming MIDI is routed.
    ///
    /// A module only reacts to MIDI itself when the handle-internal bit is
    /// set, which is what `midi bind` needs.
    RxMode {
        /// Routing type: 0 = MIDI voice, 1 = sysex, 2 = realtime, 3 = event view.
        #[arg(long, default_value_t = 0)]
        rx_type: u8,
        /// Bitmask: 1 = forward from USB, 2 = handle external, 4 = handle internal.
        mode: u8,
        /// Also store it in the module's system init script, so the routing
        /// survives a power cycle.
        #[arg(long)]
        persist: bool,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// List every Intech module this tool knows, with its layout.
    Models {
        /// Show only modules whose element layout is supported.
        #[arg(long)]
        supported: bool,
    },
    /// Live and stored control of how elements behave.
    Live {
        #[command(subcommand)]
        command: LiveCommand,
    },
    /// Inspect and update module firmware.
    Firmware {
        #[command(subcommand)]
        command: FirmwareCommand,
    },
    /// Run a Lua snippet on the module immediately.
    Exec {
        /// Lua source, e.g. `ele[0]:glc(2,{{255,0,0,1}})`.
        lua: String,
    },
    /// Persist the active page to the module's memory.
    Store,
    /// Discard unsaved changes to the active page.
    Discard,
    /// Reset the active page to firmware defaults.
    ClearPage {
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Reboot the module.
    Reset,
}

#[derive(Subcommand)]
enum LedCommand {
    /// Set an LED colour.
    Set {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// Colour, e.g. `red`, `#ff8800` or `255,136,0`.
        color: String,
        /// Brightness multiplier applied on the device, 0.0 to 1.0.
        #[arg(long, default_value_t = 1.0)]
        alpha: f32,
        /// Light the LED now instead of changing stored configuration.
        #[arg(long)]
        live: bool,
        /// LED layer to write when using --live.
        #[arg(long)]
        layer: Option<u8>,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Show stored LED colours.
    Get {
        /// Element selector. Defaults to all elements with an LED.
        #[arg(default_value = "all")]
        element: Selector,
        /// Page to read. Defaults to the active page.
        #[arg(long)]
        page: Option<u8>,
    },
    /// Reset LED colours to the module default.
    Clear {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Turn LEDs off.
    Off {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// Turn it off now instead of changing stored configuration.
        #[arg(long)]
        live: bool,
        #[command(flatten)]
        write: WriteArgs,
    },
}

#[derive(Subcommand)]
enum ValueCommand {
    /// Assign an element's value, so the next physical move continues from it.
    Set {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// New value, in the element's own min/max range (or a percentage
        /// with --percent).
        value: f32,
        /// Treat the value as a percentage (0-100) of the MIDI range.
        #[arg(long)]
        percent: bool,
        /// MIDI channel to send on. Must match the element's binding.
        #[arg(long, default_value_t = 0)]
        channel: u8,
        /// CC number of element 0; element N uses cc-base + N. Must match
        /// the binding written by `midi bind`.
        #[arg(long, default_value_t = 20)]
        cc_base: u8,
        /// Use an immediate Lua script instead of MIDI.
        ///
        /// Unverified on this hardware: nothing observable on this module
        /// responds to immediate scripts, so prefer the MIDI path.
        #[arg(long)]
        via_immediate: bool,
    },
}

#[derive(Subcommand)]
enum MidiCommand {
    /// Make elements follow an incoming MIDI message.
    Bind {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// MIDI channel. Defaults to the firmware's automatic channel.
        #[arg(long)]
        channel: Option<u8>,
        /// MIDI command byte, e.g. 176 for control change. Defaults to the
        /// firmware's automatic command for the element kind.
        #[arg(long)]
        command: Option<u8>,
        /// CC or note number. Defaults to the firmware's automatic number,
        /// which is derived from the element index.
        #[arg(long, conflicts_with = "cc_base")]
        cc: Option<u8>,
        /// Give each element its own CC: element N listens on cc-base + N.
        /// This is what `value set` sends to by default.
        #[arg(long)]
        cc_base: Option<u8>,
        /// Scale the LED only, leaving the element's value untouched.
        #[arg(long)]
        led_only: bool,
        /// Move the value only, leaving the LED untouched.
        #[arg(long, conflicts_with = "led_only")]
        value_only: bool,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Remove a binding previously written by `midi bind`.
    Unbind {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Show the bindings stored on elements.
    Show {
        /// Element selector. Defaults to every element with an LED.
        #[arg(default_value = "all")]
        element: Selector,
        /// Page to read. Defaults to the active page.
        #[arg(long)]
        page: Option<u8>,
    },
}

#[derive(Subcommand)]
enum LiveCommand {
    /// Show an element's stored behaviour settings.
    Show {
        /// Element selector: index, range, list or 'all'.
        #[arg(default_value = "all")]
        element: Selector,
        /// Page to read. Defaults to the active page.
        #[arg(long)]
        page: Option<u8>,
    },
    /// Set how a knob turns movement into a value.
    Mode {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// absolute, relative, or relative-signed.
        mode: KnobMode,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Set an LED animation: rate and type, both 0 for steady.
    Anim {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        /// Animation rate (firmware `glf`). 0 is steady.
        rate: u8,
        /// Animation type (firmware `gls`). 0 is steady.
        #[arg(default_value_t = 0)]
        shape: u8,
        /// LED layer to write. Defaults to the element's own layer.
        #[arg(long)]
        layer: Option<u8>,
        /// Drive it now instead of storing it.
        #[arg(long)]
        live: bool,
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Remove a stored LED animation.
    AnimClear {
        /// Element selector: index, range, list or 'all'.
        element: Selector,
        #[command(flatten)]
        write: WriteArgs,
    },
}

#[derive(Subcommand)]
enum FirmwareCommand {
    /// Show the connected module's firmware and any bootloader volume.
    Status,
    /// Validate a .uf2 firmware image without writing anything.
    Validate {
        /// Path to the .uf2 image.
        file: PathBuf,
    },
    /// Flash a .uf2 image onto a module in bootloader mode.
    ///
    /// Destructive and irreversible. Requires --yes.
    Update {
        /// Path to the .uf2 image.
        file: PathBuf,
        /// Confirm the flash. Without it, nothing is written.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args)]
struct WriteArgs {
    /// Page to modify. Defaults to the active page, which is the only page
    /// the firmware accepts configuration writes for.
    #[arg(long)]
    page: Option<u8>,
    /// Keep the change in RAM instead of persisting it to the module.
    #[arg(long)]
    no_store: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("gridctl: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Resolve an optional `--page` against the module's active page.
///
/// The firmware refuses configuration writes for any page other than the
/// active one, so defaulting to it is the only safe default.
fn resolve_page(dev: &mut Device, page: Option<u8>) -> Result<u8, Box<dyn std::error::Error>> {
    match page {
        Some(p) => Ok(p),
        None => Ok(dev.active_page()?),
    }
}

fn open(cli: &Cli) -> Result<Device, Box<dyn std::error::Error>> {
    Ok(match &cli.port {
        Some(p) => Device::open(p)?,
        None => Device::open_single()?,
    })
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.command {
        Command::List => cmd_list(cli),
        Command::Info => cmd_info(cli),
        Command::Led { command } => match command {
            LedCommand::Set {
                element,
                color,
                alpha,
                live,
                layer,
                write,
            } => cmd_led_set(cli, element, color, *alpha, *live, *layer, write),
            LedCommand::Get { element, page } => cmd_led_get(cli, element, *page),
            LedCommand::Clear { element, write } => cmd_led_clear(cli, element, write),
            LedCommand::Off {
                element,
                live,
                write,
            } => cmd_led_off(cli, element, *live, write),
        },
        Command::Value { command } => match command {
            ValueCommand::Set {
                element,
                value,
                percent,
                channel,
                cc_base,
                via_immediate,
            } => cmd_value_set(
                cli,
                element,
                *value,
                *percent,
                *channel,
                *cc_base,
                *via_immediate,
            ),
        },
        Command::Midi { command } => match command {
            MidiCommand::Bind {
                element,
                channel,
                command,
                cc,
                cc_base,
                led_only,
                value_only,
                write,
            } => cmd_midi_bind(
                cli,
                element,
                *channel,
                *command,
                *cc,
                *cc_base,
                *led_only,
                *value_only,
                write,
            ),
            MidiCommand::Unbind { element, write } => cmd_midi_unbind(cli, element, write),
            MidiCommand::Show { element, page } => cmd_midi_show(cli, element, *page),
        },
        Command::RxMode {
            rx_type,
            mode,
            persist,
            write,
        } => cmd_rx_mode(cli, *rx_type, *mode, *persist, write),
        Command::Get { element, page, raw } => cmd_get(cli, *element, *page, *raw),
        Command::Set {
            element,
            name,
            value,
            write,
        } => cmd_set(cli, element, name, *value, write),
        Command::Dump { page, output } => cmd_dump(cli, page, output.as_deref()),
        Command::Load { file, write } => cmd_load(cli, file, write),
        Command::Models { supported } => cmd_models(cli, *supported),
        Command::Live { command } => match command {
            LiveCommand::Show { element, page } => cmd_live_show(cli, element, *page),
            LiveCommand::Mode {
                element,
                mode,
                write,
            } => cmd_live_mode(cli, element, *mode, write),
            LiveCommand::Anim {
                element,
                rate,
                shape,
                layer,
                live,
                write,
            } => cmd_live_anim(cli, element, *rate, *shape, *layer, *live, write),
            LiveCommand::AnimClear { element, write } => cmd_live_anim_clear(cli, element, write),
        },
        Command::Firmware { command } => match command {
            FirmwareCommand::Status => cmd_fw_status(cli),
            FirmwareCommand::Validate { file } => cmd_fw_validate(cli, file),
            FirmwareCommand::Update { file, yes } => cmd_fw_update(cli, file, *yes),
        },
        Command::Exec { lua } => {
            let mut dev = open(cli)?;
            dev.immediate(lua)?;
            println!("ran {} bytes of Lua", lua.len());
            Ok(())
        }
        Command::Store => {
            let mut dev = open(cli)?;
            dev.store_page()?;
            println!("stored the active page");
            Ok(())
        }
        Command::Discard => {
            let mut dev = open(cli)?;
            dev.discard_page()?;
            println!("discarded unsaved changes");
            Ok(())
        }
        Command::ClearPage { write } => {
            let mut dev = open(cli)?;
            dev.clear_page()?;
            if !write.no_store {
                dev.store_page()?;
            }
            println!("reset the active page to firmware defaults");
            Ok(())
        }
        Command::Reset => {
            let mut dev = open(cli)?;
            dev.reset()?;
            println!("module is rebooting");
            Ok(())
        }
    }
}

fn cmd_list(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let ports = orb_intech_grid::discover()?;
    if cli.json {
        let items: Vec<_> = ports
            .iter()
            .map(|p| {
                serde_json::json!({
                    "port": p.path,
                    "by_id": p.by_id,
                    "usb_serial": p.usb_serial,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    if ports.is_empty() {
        println!("no Grid modules found");
        return Ok(());
    }
    for p in &ports {
        // Identify each port, but do not fail the listing if one is busy.
        match Device::open(&p.path) {
            Ok(dev) => {
                let i = dev.info();
                println!(
                    "{}  {}  firmware {}  {} elements, {} LEDs",
                    p.path.display(),
                    i.module,
                    i.firmware,
                    i.element_count,
                    i.led_count
                );
            }
            Err(e) => println!("{}  (could not identify: {})", p.path.display(), e),
        }
    }
    Ok(())
}

fn cmd_info(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = dev.active_page().ok();
    let info = dev.info().clone();
    let module = dev.module();

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "module": info.module,
                "hwcfg": info.hwcfg,
                "firmware": info.firmware,
                "elements": info.element_count,
                "leds": info.led_count,
                "position": [info.position.0, info.position.1],
                "active_page": page,
                "port": info.port,
                "usb_serial": info.usb_serial,
            }))?
        );
        return Ok(());
    }

    println!("module      {}", info.module);
    println!("hwcfg       0x{:02x}", info.hwcfg);
    println!("firmware    {}", info.firmware);
    println!("port        {}", info.port.display());
    if let Some(s) = &info.usb_serial {
        println!("usb serial  {s}");
    }
    println!("position    {},{}", info.position.0, info.position.1);
    match page {
        Some(p) => println!("active page {p}"),
        None => println!("active page unknown"),
    }
    println!(
        "elements    {} ({} LEDs)",
        info.element_count, info.led_count
    );

    let mut runs: Vec<(ElementKind, u8, u8)> = Vec::new();
    for i in 0..module.element_count() {
        let kind = module.element_kind(i)?;
        match runs.last_mut() {
            Some((k, _, end)) if *k == kind => *end = i,
            _ => runs.push((kind, i, i)),
        }
    }
    for (kind, start, end) in runs {
        if start == end {
            println!("  {start:<7} {kind}");
        } else {
            println!("  {:<7} {kind}", format!("{start}-{end}"));
        }
    }
    Ok(())
}

/// Translate a friendly setting name into the firmware's short name.
fn setting_name(kind: ElementKind, name: &str) -> Result<&'static str, String> {
    let n = name.trim().to_ascii_lowercase();
    let resolved = match (kind, n.as_str()) {
        (ElementKind::Encoder, "min") => "emi",
        (ElementKind::Encoder, "max") => "ema",
        (ElementKind::Encoder, "mode") => "emo",
        (ElementKind::Encoder, "sensitivity") => "ese",
        (ElementKind::Encoder, "velocity") => "ev0",
        (ElementKind::Potmeter, "min") => "pmi",
        (ElementKind::Potmeter, "max") => "pma",
        (ElementKind::Potmeter, "mode" | "resolution") => "pmo",
        (ElementKind::Button, "min") => "bmi",
        (ElementKind::Button, "max") => "bma",
        (ElementKind::Button, "mode") => "bmo",
        // Already a firmware short name.
        (_, other) => {
            return match other {
                "emi" | "ema" | "emo" | "ese" | "ev0" | "pmi" | "pma" | "pmo" | "bmi" | "bma"
                | "bmo" | "bstp" => Ok(match other {
                    "emi" => "emi",
                    "ema" => "ema",
                    "emo" => "emo",
                    "ese" => "ese",
                    "ev0" => "ev0",
                    "pmi" => "pmi",
                    "pma" => "pma",
                    "pmo" => "pmo",
                    "bmi" => "bmi",
                    "bma" => "bma",
                    "bmo" => "bmo",
                    _ => "bstp",
                }),
                _ => Err(format!(
                    "unknown setting {name:?} for a {kind}; try min, max, mode, sensitivity or velocity"
                )),
            }
        }
    };
    Ok(resolved)
}

fn cmd_led_set(
    cli: &Cli,
    element: &Selector,
    color: &str,
    alpha: f32,
    live: bool,
    layer: Option<u8>,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let rgb: Rgb = color.parse()?;
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;

    for &e in &targets {
        if live {
            dev.flash_led(e, rgb.scaled(alpha), layer)?;
        } else {
            dev.set_led_color(page, e, rgb, alpha)?;
        }
    }

    // Live changes never touch stored config, and --no-store deliberately
    // leaves the change in RAM.
    let persisted = if live || write.no_store {
        false
    } else {
        dev.store_page()?;
        true
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "elements": targets,
                "color": rgb.to_hex(),
                "alpha": alpha,
                "live": live,
                "persisted": persisted,
            }))?
        );
    } else {
        let what = if live { "lit" } else { "set" };
        println!(
            "{what} {} element(s) to {}{}",
            targets.len(),
            rgb.to_hex(),
            if live {
                " (live, until the element is touched)"
            } else if persisted {
                " and stored it on the module"
            } else {
                " in RAM (run 'gridctl store' to keep it)"
            }
        );
    }
    Ok(())
}

fn cmd_led_get(
    cli: &Cli,
    element: &Selector,
    page: Option<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, page)?;
    let targets = element.resolve(dev.module())?;

    let mut rows = Vec::new();
    for &e in &targets {
        let color = dev.led_color(page, e)?;
        rows.push((e, color));
    }

    if cli.json {
        let items: Vec<_> = rows
            .iter()
            .map(|(e, c)| {
                serde_json::json!({
                    "element": e,
                    "color": c.map(|c| c.to_hex()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    for (e, color) in rows {
        match color {
            Some(c) => println!("{e:<3} {}", c.to_hex()),
            None => println!("{e:<3} (module default)"),
        }
    }
    Ok(())
}

fn cmd_led_clear(
    cli: &Cli,
    element: &Selector,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;
    for &e in &targets {
        dev.clear_led_color(page, e)?;
    }
    if !write.no_store {
        dev.store_page()?;
    }
    println!(
        "reset {} element(s) to the module default colour",
        targets.len()
    );
    Ok(())
}

fn cmd_led_off(
    cli: &Cli,
    element: &Selector,
    live: bool,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let targets = element.resolve(dev.module())?;

    if live {
        for &e in &targets {
            dev.flash_led_off(e)?;
        }
        println!(
            "turned {} LED(s) off now (until the element is touched)",
            targets.len()
        );
        return Ok(());
    }

    let page = resolve_page(&mut dev, write.page)?;
    let mut written = 0;
    let mut changed = Vec::new();
    for &e in &targets {
        let n = dev.set_led_off(page, e)?;
        if n > 0 {
            written += n;
            changed.push(e);
        }
    }

    if written == 0 {
        println!(
            "no LED colour found to change on {} element(s); they were already \
             black or their scripts do not set a colour",
            targets.len()
        );
        return Ok(());
    }
    if !write.no_store {
        dev.store_page()?;
    }
    println!(
        "turned {} element(s) off ({written} script(s) rewritten){}",
        changed.len(),
        if write.no_store {
            " in RAM (run 'gridctl store' to keep it)"
        } else {
            " and stored it"
        }
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_value_set(
    cli: &Cli,
    element: &Selector,
    value: f32,
    percent: bool,
    channel: u8,
    cc_base: u8,
    via_immediate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let targets = element.resolve(dev.module())?;

    if via_immediate {
        for &e in &targets {
            if percent {
                dev.set_element_percent(e, value)?;
            } else {
                if !(0.0..=255.0).contains(&value) {
                    return Err(format!("value {value} is outside 0-255").into());
                }
                dev.set_element_value(e, value.round() as u8)?;
            }
        }
        println!(
            "sent {} immediate script(s) (unverified path; prefer MIDI)",
            targets.len()
        );
        return Ok(());
    }

    // MIDI is the proven transport: with a binding in place the firmware
    // moves the element's value and scales its LED from the received value.
    let midi_value = if percent {
        if !(0.0..=100.0).contains(&value) {
            return Err(format!("percentage {value} is outside 0-100").into());
        }
        (value / 100.0 * 127.0).round() as u8
    } else {
        if !(0.0..=127.0).contains(&value) {
            return Err(format!("value {value} is outside the MIDI range 0-127").into());
        }
        value.round() as u8
    };

    let mut out = orb_intech_grid::MidiOut::open_single()?;
    let mut sent = Vec::new();
    for &e in &targets {
        let cc = cc_base
            .checked_add(e)
            .ok_or_else(|| format!("cc-base {cc_base} + element {e} exceeds 255"))?;
        out.control_change(channel, cc, midi_value)?;
        sent.push(cc.to_string());
    }

    println!(
        "sent CC {} = {midi_value} on channel {channel} via {}",
        sent.join(","),
        out.path().display()
    );
    println!(
        "the element follows this only while it is bound (gridctl midi bind) \
         and handle-internal routing is on (gridctl rx-mode 4 --persist)"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_midi_bind(
    cli: &Cli,
    element: &Selector,
    channel: Option<u8>,
    command: Option<u8>,
    cc: Option<u8>,
    cc_base: Option<u8>,
    led_only: bool,
    value_only: bool,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;

    let mut binding = orb_intech_grid::MidiRxBinding {
        channel,
        command,
        parameter: cc,
        set_value: !led_only,
        set_led: !value_only,
    };

    for &e in &targets {
        if let Some(base) = cc_base {
            // One CC per element, which is what `value set` addresses.
            binding.parameter = Some(
                base.checked_add(e)
                    .ok_or_else(|| format!("cc-base {base} + element {e} exceeds 255"))?,
            );
        }
        dev.bind_midi_rx(page, e, &binding)?;
    }
    if !write.no_store {
        dev.store_page()?;
    }

    println!(
        "bound {} element(s) to MIDI ({}{}){}",
        targets.len(),
        if binding.set_value { "value" } else { "" },
        if binding.set_value && binding.set_led {
            " + LED"
        } else if binding.set_led {
            "LED"
        } else {
            ""
        },
        if write.no_store {
            " in RAM"
        } else {
            " and stored it"
        }
    );
    println!(
        "note: the module only acts on MIDI when handle-internal routing is on \
         (gridctl rx-mode 4 --persist)"
    );
    Ok(())
}

fn cmd_midi_unbind(
    cli: &Cli,
    element: &Selector,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;

    let mut removed = 0;
    for &e in &targets {
        if dev.unbind_midi_rx(page, e)? {
            removed += 1;
        }
    }
    if removed > 0 && !write.no_store {
        dev.store_page()?;
    }
    println!("removed {removed} binding(s)");
    Ok(())
}

fn cmd_midi_show(
    cli: &Cli,
    element: &Selector,
    page: Option<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, page)?;
    let targets = element.resolve(dev.module())?;

    let mut rows = Vec::new();
    for &e in &targets {
        rows.push((e, dev.midi_rx_binding(page, e)?));
    }

    if cli.json {
        let items: Vec<_> = rows
            .iter()
            .map(|(e, b)| serde_json::json!({ "element": e, "binding": b }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    for (e, b) in rows {
        match b {
            Some(lua) => println!("{e:<3} {lua}"),
            None => println!("{e:<3} (none)"),
        }
    }
    Ok(())
}

fn cmd_rx_mode(
    cli: &Cli,
    rx_type: u8,
    mode: u8,
    persist: bool,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    dev.set_rx_mode(rx_type, mode)?;

    if persist {
        let page = resolve_page(&mut dev, write.page)?;
        dev.persist_rx_mode(page, rx_type, mode)?;
        if !write.no_store {
            dev.store_page()?;
        }
    }

    let mut flags = Vec::new();
    if mode & 0x01 != 0 {
        flags.push("forward-from-usb");
    }
    if mode & 0x02 != 0 {
        flags.push("handle-external");
    }
    if mode & 0x04 != 0 {
        flags.push("handle-internal");
    }
    println!(
        "rx_mode({rx_type}) = {mode} [{}]{}",
        if flags.is_empty() {
            "off".to_string()
        } else {
            flags.join(", ")
        },
        if persist {
            ", persisted"
        } else {
            " (this session only)"
        }
    );
    Ok(())
}

fn cmd_get(
    cli: &Cli,
    element: u8,
    page: Option<u8>,
    raw: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, page)?;
    let kind = dev.module().element_kind(element)?;
    let (event, action) = dev.element_action(page, element)?;

    let named: &[(&str, &str)] = match kind {
        ElementKind::Encoder => &[
            ("min", "emi"),
            ("max", "ema"),
            ("mode", "emo"),
            ("sensitivity", "ese"),
            ("velocity", "ev0"),
        ],
        ElementKind::Potmeter => &[("min", "pmi"), ("max", "pma"), ("mode", "pmo")],
        ElementKind::Button => &[("min", "bmi"), ("max", "bma"), ("mode", "bmo")],
        _ => &[],
    };

    if cli.json {
        let mut settings = serde_json::Map::new();
        for (label, short) in named {
            settings.insert(
                (*label).to_string(),
                match action.setting(short) {
                    Some(v) => serde_json::json!(v),
                    None => serde_json::Value::Null,
                },
            );
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "element": element,
                "kind": kind.as_str(),
                "event": event.as_str(),
                "led_color": action.led_color().map(|c| c.to_hex()),
                "settings": settings,
                "action": action.render(),
            }))?
        );
        return Ok(());
    }

    println!("element {element} ({kind}), page {page}, event {event}");
    match action.led_color() {
        Some(c) => println!("  colour       {}", c.to_hex()),
        None => println!("  colour       (module default)"),
    }
    for (label, short) in named {
        match action.setting(short) {
            Some(v) => println!("  {label:<12} {v}"),
            None => println!("  {label:<12} (unset)"),
        }
    }
    if raw {
        for &ev in kind.events() {
            let text = dev.fetch_action(page, element, ev)?;
            println!("  [{}] {}", ev.as_str(), text);
        }
    }
    Ok(())
}

fn cmd_set(
    cli: &Cli,
    element: &Selector,
    name: &str,
    value: i64,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;

    for &e in &targets {
        let kind = dev.module().element_kind(e)?;
        let short = setting_name(kind, name)?;
        dev.set_element_setting(page, e, short, value)?;
    }
    if !write.no_store {
        dev.store_page()?;
    }
    println!("set {name}={value} on {} element(s)", targets.len());
    Ok(())
}

fn cmd_dump(
    cli: &Cli,
    pages: &[u8],
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let snap = snapshot::dump(&mut dev, pages)?;
    let json = snap.to_json()?;
    match output {
        Some(path) => {
            std::fs::write(path, format!("{json}\n"))?;
            eprintln!("wrote {}", path.display());
        }
        None => println!("{json}"),
    }
    Ok(())
}

fn cmd_load(
    cli: &Cli,
    file: &std::path::Path,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = if file == std::path::Path::new("-") {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s
    } else {
        std::fs::read_to_string(file)?
    };

    let snap = orb_intech_grid::Snapshot::from_json(&text)?;
    let mut dev = open(cli)?;
    if snap.hwcfg != dev.info().hwcfg {
        return Err(format!(
            "snapshot is for {} (hwcfg 0x{:02x}) but the connected module is {} (hwcfg 0x{:02x})",
            snap.module,
            snap.hwcfg,
            dev.info().module,
            dev.info().hwcfg
        )
        .into());
    }

    let report = snapshot::apply(&mut dev, &snap)?;
    if !write.no_store && !report.written.is_empty() {
        dev.store_page()?;
    }
    println!(
        "wrote {} script(s), {} already matched{}",
        report.written.len(),
        report.unchanged,
        if write.no_store { " (kept in RAM)" } else { "" }
    );
    Ok(())
}


/// `gridctl models` - the model table, which needs no device.
fn cmd_models(cli: &Cli, supported_only: bool) -> Result<(), Box<dyn std::error::Error>> {
    use orb_intech_grid::module::MODULES;

    let rows: Vec<_> = MODULES
        .iter()
        .filter(|m| !supported_only || m.layout_known())
        .collect();

    if cli.json {
        let items: Vec<_> = rows
            .iter()
            .map(|m| {
                serde_json::json!({
                    "hwcfg": m.hwcfg,
                    "name": m.name(),
                    "kind": m.kind.as_str(),
                    "revision": m.revision,
                    "variant": m.variant,
                    "description": m.kind.describe(),
                    "leds": m.led_count,
                    "elements": m.element_count(),
                    "supported": m.layout_known(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    println!(
        "{:<5} {:<14} {:<5} {:<5} {:<9} CONTROLS",
        "HWCFG", "MODULE", "ELEM", "LEDS", "SUPPORT"
    );
    for m in &rows {
        println!(
            "{:<5} {:<14} {:<5} {:<5} {:<9} {}",
            format!("0x{:02x}", m.hwcfg),
            m.name(),
            if m.layout_known() {
                m.element_count().to_string()
            } else {
                "-".into()
            },
            if m.layout_known() {
                m.led_count.to_string()
            } else {
                "-".into()
            },
            if m.layout_known() { "yes" } else { "no" },
            m.kind.describe()
        );
    }
    let known = rows.iter().filter(|m| m.layout_known()).count();
    println!(
        "\n{} hardware ids, {known} with a supported element layout.",
        rows.len()
    );
    if !supported_only && known < rows.len() {
        println!(
            "Unsupported ids have no element layout in the firmware either - it logs \
             \"Unknown Module\" for them."
        );
    }
    Ok(())
}

fn cmd_live_show(
    cli: &Cli,
    element: &Selector,
    page: Option<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, page)?;
    let targets = element.resolve(dev.module())?;

    let mut rows = Vec::new();
    for &e in &targets {
        let kind = dev.module().element_kind(e)?;
        rows.push((e, kind, dev.element_settings(page, e)?));
    }

    if cli.json {
        let items: Vec<_> = rows
            .iter()
            .map(|(e, kind, s)| {
                serde_json::json!({
                    "element": e,
                    "kind": kind.as_str(),
                    "mode": s.mode.map(|m| m.as_str()),
                    "mode_raw": s.mode_raw,
                    "min": s.min,
                    "max": s.max,
                    "sensitivity": s.sensitivity,
                    "velocity": s.velocity,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    let f = |v: Option<i64>| match v {
        Some(x) => x.to_string(),
        None => "default".to_string(),
    };
    println!(
        "{:<4} {:<9} {:<16} {:<8} {:<8} {:<6} VEL",
        "EL", "KIND", "MODE", "MIN", "MAX", "SENS"
    );
    for (e, kind, s) in rows {
        let mode = match (s.mode, s.mode_raw) {
            (Some(m), _) => m.as_str().to_string(),
            (None, Some(raw)) => format!("unknown({raw})"),
            (None, None) => "default".to_string(),
        };
        println!(
            "{e:<4} {:<9} {mode:<16} {:<8} {:<8} {:<6} {}",
            kind.as_str(),
            f(s.min),
            f(s.max),
            f(s.sensitivity),
            f(s.velocity)
        );
    }
    Ok(())
}

fn cmd_live_mode(
    cli: &Cli,
    element: &Selector,
    mode: KnobMode,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;

    // Only knobs have a mode; skip the rest rather than failing the batch.
    let mut changed = Vec::new();
    let mut skipped = Vec::new();
    for &e in &targets {
        match dev.module().element_kind(e)? {
            ElementKind::Encoder | ElementKind::Endless => {
                dev.set_knob_mode(page, e, mode)?;
                changed.push(e);
            }
            other => skipped.push((e, other)),
        }
    }
    if !changed.is_empty() && !write.no_store {
        dev.store_page()?;
    }
    println!(
        "set {} element(s) to {mode}{}",
        changed.len(),
        if write.no_store {
            " in RAM (run 'gridctl store' to keep it)"
        } else {
            " and stored it"
        }
    );
    if !skipped.is_empty() {
        println!(
            "skipped {} element(s) with no movement mode: {}",
            skipped.len(),
            skipped
                .iter()
                .map(|(e, k)| format!("{e} ({k})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_live_anim(
    cli: &Cli,
    element: &Selector,
    rate: u8,
    shape: u8,
    layer: Option<u8>,
    live: bool,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let anim = LedAnimation::new(rate, shape);
    let mut dev = open(cli)?;
    let targets = element.resolve(dev.module())?;

    if live {
        for &e in &targets {
            dev.flash_led_animation(e, anim, layer)?;
        }
        println!(
            "drove animation rate={rate} type={shape} on {} element(s) now",
            targets.len()
        );
        return Ok(());
    }

    let page = resolve_page(&mut dev, write.page)?;
    for &e in &targets {
        dev.set_led_animation(page, e, anim, layer)?;
    }
    if !write.no_store {
        dev.store_page()?;
    }
    println!(
        "stored animation rate={rate} type={shape} on {} element(s){}",
        targets.len(),
        if write.no_store { " in RAM" } else { " and saved it" }
    );
    Ok(())
}

fn cmd_live_anim_clear(
    cli: &Cli,
    element: &Selector,
    write: &WriteArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dev = open(cli)?;
    let page = resolve_page(&mut dev, write.page)?;
    let targets = element.resolve(dev.module())?;

    let mut removed = 0;
    for &e in &targets {
        if dev.clear_led_animation(page, e)? {
            removed += 1;
        }
    }
    if removed > 0 && !write.no_store {
        dev.store_page()?;
    }
    println!("removed {removed} stored animation(s)");
    Ok(())
}

fn cmd_fw_status(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let volumes = firmware::find_bootloader_volumes()?;

    // A module in bootloader mode has no serial port, so a missing device is
    // expected and must not be reported as an error here.
    let device = match open(cli) {
        Ok(dev) => {
            let i = dev.info().clone();
            Some((i.module, i.firmware, i.port))
        }
        Err(_) => None,
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "device": device.as_ref().map(|(m, f, p)| serde_json::json!({
                    "module": m, "firmware": f, "port": p,
                })),
                "bootloader_volumes": volumes
                    .iter()
                    .map(|v| serde_json::json!({
                        "mount": v.mount_point,
                        "has_info_file": v.has_info_file,
                    }))
                    .collect::<Vec<_>>(),
            }))?
        );
        return Ok(());
    }

    match &device {
        Some((m, f, p)) => println!("running    {m}, firmware {f} ({})", p.display()),
        None => println!("running    no module responding on a serial port"),
    }
    if volumes.is_empty() {
        println!("bootloader no UF2 volume mounted");
        println!("\n{}", firmware::BOOTLOADER_INSTRUCTIONS);
    } else {
        for v in &volumes {
            println!(
                "bootloader {} (INFO_UF2.TXT: {})",
                v.mount_point.display(),
                if v.has_info_file { "yes" } else { "no" }
            );
        }
        println!("\nready to flash: gridctl firmware update <image.uf2> --yes");
    }
    Ok(())
}

fn cmd_fw_validate(cli: &Cli, file: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let img = firmware::Uf2Image::open(file)?;
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": img.path,
                "valid": true,
                "blocks": img.blocks,
                "bytes": img.bytes,
                "payload_bytes": img.payload_bytes,
                "family_id": format!("0x{:08x}", img.family_id),
                "target_address": format!("0x{:x}", img.target_address),
            }))?
        );
        return Ok(());
    }
    println!("valid UF2 image for ESP32-S3 (family 0x{:08x})", img.family_id);
    println!("  path          {}", img.path.display());
    println!("  blocks        {}", img.blocks);
    println!("  size          {} bytes", img.bytes);
    println!("  payload       {} bytes", img.payload_bytes);
    println!("  first target  0x{:x}", img.target_address);
    Ok(())
}

fn cmd_fw_update(
    cli: &Cli,
    file: &std::path::Path,
    yes: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate before anything else: a bad image must never reach the volume.
    let img = firmware::Uf2Image::open(file)?;
    let volumes = firmware::find_bootloader_volumes()?;

    let volume = match volumes.len() {
        0 => {
            return Err(format!(
                "no UF2 bootloader volume is mounted.\n\n{}",
                firmware::BOOTLOADER_INSTRUCTIONS
            )
            .into())
        }
        1 => &volumes[0],
        n => {
            return Err(format!(
                "{n} UF2 volumes are mounted; unplug the others so the target is unambiguous"
            )
            .into())
        }
    };

    if !yes {
        println!("would flash {} ({} bytes)", img.path.display(), img.bytes);
        println!("         to {}", volume.mount_point.display());
        println!("\nThis overwrites the module's firmware and cannot be undone.");
        println!("Re-run with --yes to proceed.");
        return Ok(());
    }

    let written = firmware::flash(volume, &img)?;
    println!(
        "flashed {} ({written} bytes) to {}",
        img.path.display(),
        volume.mount_point.display()
    );
    println!("the module reboots into the new firmware; re-run 'gridctl firmware status' to confirm");
    let _ = cli;
    Ok(())
}

/// Keep the parser honest: clap panics on a malformed command tree.
#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_a_persistent_colour_change() {
        let cli = Cli::try_parse_from(["gridctl", "led", "set", "3", "red"]).unwrap();
        match cli.command {
            Command::Led {
                command:
                    LedCommand::Set {
                        ref element,
                        ref color,
                        live,
                        ref write,
                        ..
                    },
            } => {
                assert_eq!(*element, Selector::List(vec![3]));
                assert_eq!(color, "red");
                assert!(!live, "persistent is the default");
                assert!(!write.no_store, "persisting is the default");
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn parses_live_and_all() {
        let cli =
            Cli::try_parse_from(["gridctl", "led", "set", "all", "#ff8800", "--live"]).unwrap();
        match cli.command {
            Command::Led {
                command: LedCommand::Set { element, live, .. },
            } => {
                assert_eq!(element, Selector::All);
                assert!(live);
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn models_needs_no_device_and_lists_everything() {
        // The model table is static, so this command must parse and run
        // without any hardware attached.
        let cli = Cli::try_parse_from(["gridctl", "models"]).unwrap();
        assert!(matches!(cli.command, Command::Models { supported: false }));
        cmd_models(&cli, false).expect("models runs with no device");
        cmd_models(&cli, true).expect("supported-only runs with no device");
    }

    #[test]
    fn parses_knob_mode_words() {
        let cli = Cli::try_parse_from(["gridctl", "live", "mode", "all", "relative"]).unwrap();
        match cli.command {
            Command::Live {
                command: LiveCommand::Mode { mode, .. },
            } => assert_eq!(mode, KnobMode::Relative),
            _ => panic!("wrong command"),
        }
        assert!(Cli::try_parse_from(["gridctl", "live", "mode", "0", "sideways"]).is_err());
    }

    #[test]
    fn firmware_update_requires_explicit_confirmation() {
        let cli = Cli::try_parse_from(["gridctl", "firmware", "update", "x.uf2"]).unwrap();
        match cli.command {
            Command::Firmware {
                command: FirmwareCommand::Update { yes, .. },
            } => assert!(!yes, "flashing must not be the default"),
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn maps_friendly_setting_names() {
        assert_eq!(setting_name(ElementKind::Encoder, "max").unwrap(), "ema");
        assert_eq!(setting_name(ElementKind::Encoder, "MIN").unwrap(), "emi");
        assert_eq!(setting_name(ElementKind::Potmeter, "max").unwrap(), "pma");
        assert_eq!(setting_name(ElementKind::Button, "mode").unwrap(), "bmo");
        assert_eq!(setting_name(ElementKind::Encoder, "ema").unwrap(), "ema");
        assert!(setting_name(ElementKind::Encoder, "colour").is_err());
    }
}
