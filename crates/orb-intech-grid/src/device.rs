//! Talking to a Grid module over its USB serial port.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::action::{
    ActionString, ANIMATION_BLOCK, BUTTON_BLOCK, ENCODER_BLOCK, POTMETER_BLOCK,
};
use crate::live::{ElementSettings, KnobMode, LedAnimation};
use crate::color::Rgb;
use crate::error::{Error, Result};
use crate::frame::{self, Body, Frame, Header};
use crate::module::{ElementKind, Module};
use crate::proto::{
    class, config_fields, heartbeat_fields, instr, layer, EventType, ACTIONSTRING_MAXLENGTH,
    USB_PID, USB_VID,
};

/// Default time to wait for a reply from the device.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5000);

/// Whether to trace every packet to stderr, via `GRID_DEBUG=1`.
fn debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GRID_DEBUG").is_some())
}

/// Verdict of a reply matcher, used to correlate replies with requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Match {
    /// This is the reply we asked for.
    Yes,
    /// Not our reply; keep waiting.
    No,
    /// Our request, explicitly refused by the device.
    Rejected,
}

/// A Grid serial port found on the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// Device node, e.g. `/dev/ttyACM0`.
    pub path: PathBuf,
    /// Stable `/dev/serial/by-id` symlink, when one exists.
    pub by_id: Option<PathBuf>,
    /// USB serial string, when it could be determined.
    pub usb_serial: Option<String>,
}

/// What a module reports about itself in its heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DeviceInfo {
    pub module: String,
    pub hwcfg: u8,
    pub firmware: String,
    pub element_count: u8,
    pub led_count: u8,
    /// Module position on the bus, `127,127` for a standalone module.
    pub position: (u8, u8),
    pub port: PathBuf,
    pub usb_serial: Option<String>,
}

/// Find every Grid module connected over USB serial.
///
/// Prefers the stable `/dev/serial/by-id` symlinks, which encode the vendor
/// string, and falls back to matching USB ids through sysfs.
pub fn discover() -> Result<Vec<PortInfo>> {
    let mut found: Vec<PortInfo> = Vec::new();

    let by_id_dir = Path::new("/dev/serial/by-id");
    if by_id_dir.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(by_id_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for link in entries {
            let name = link
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if !name.to_ascii_lowercase().contains("intech") {
                continue;
            }
            let target = std::fs::canonicalize(&link)?;
            let usb_serial = name
                .rsplit_once("-if")
                .map(|(head, _)| head)
                .unwrap_or(&name)
                .rsplit('_')
                .next()
                .map(|s| s.to_string());
            found.push(PortInfo {
                path: target,
                by_id: Some(link),
                usb_serial,
            });
        }
    }

    if found.is_empty() {
        // Fall back to enumerating ports and checking USB ids.
        for port in serialport::available_ports().unwrap_or_default() {
            if let serialport::SerialPortType::UsbPort(usb) = &port.port_type {
                if usb.vid == USB_VID && usb.pid == USB_PID {
                    found.push(PortInfo {
                        path: PathBuf::from(&port.port_name),
                        by_id: None,
                        usb_serial: usb.serial_number.clone(),
                    });
                }
            }
        }
    }

    Ok(found)
}

/// An open connection to one Grid module.
pub struct Device {
    port: Box<dyn serialport::SerialPort>,
    /// Bytes read but not yet consumed as whole packets.
    pending: Vec<u8>,
    session: u8,
    next_id: u8,
    module: &'static Module,
    info: DeviceInfo,
    timeout: Duration,
}

impl Device {
    /// Open the only connected module, erroring when there is not exactly one.
    pub fn open_single() -> Result<Self> {
        let ports = discover()?;
        match ports.len() {
            0 => Err(Error::NoDevice {
                vid: USB_VID,
                pid: USB_PID,
            }),
            1 => Device::open(&ports[0].path),
            n => Err(Error::Other(format!(
                "{n} Grid modules are connected; pass --port to pick one ({})",
                ports
                    .iter()
                    .map(|p| p.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// Open a specific serial port and wait for the module to identify itself.
    pub fn open(path: &Path) -> Result<Self> {
        let port = serialport::new(path.to_string_lossy(), 2_000_000)
            .timeout(Duration::from_millis(50))
            .open()?;

        // The session byte only has to be stable and distinct from other hosts.
        let session = {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(1);
            ((nanos % 255) as u8).max(1)
        };

        let mut dev = Device {
            port,
            pending: Vec::new(),
            session,
            next_id: 1,
            // Replaced immediately below; EN16 RevD is just a placeholder.
            module: Module::from_hwcfg(193)?,
            info: DeviceInfo {
                module: String::new(),
                hwcfg: 0,
                firmware: String::new(),
                element_count: 0,
                led_count: 0,
                position: (0, 0),
                port: path.to_path_buf(),
                usb_serial: None,
            },
            timeout: DEFAULT_TIMEOUT,
        };

        let usb_serial = discover()?
            .into_iter()
            .find(|p| p.path == path)
            .and_then(|p| p.usb_serial);

        let frame = dev.wait_for_body(class::HEARTBEAT, |_| Match::Yes, "heartbeat")?;
        let hb = frame.body(class::HEARTBEAT).expect("matched above");
        let hwcfg = hb.field_u8(heartbeat_fields::HWCFG)?;
        let module = Module::from_hwcfg(hwcfg)?;
        let firmware = format!(
            "{}.{}.{}",
            hb.field_u8(heartbeat_fields::VERSION_MAJOR)?,
            hb.field_u8(heartbeat_fields::VERSION_MINOR)?,
            hb.field_u8(heartbeat_fields::VERSION_PATCH)?
        );

        dev.module = module;
        dev.info = DeviceInfo {
            module: module.name(),
            hwcfg,
            firmware,
            element_count: module.element_count(),
            led_count: module.led_count,
            position: (frame.header.sx, frame.header.sy),
            port: path.to_path_buf(),
            usb_serial,
        };
        Ok(dev)
    }

    /// How long to wait for replies.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn module(&self) -> &'static Module {
        self.module
    }

    /// Firmware version as a `(major, minor, patch)` triple.
    fn firmware_triple(&self) -> (u8, u8, u8) {
        let mut it = self
            .info
            .firmware
            .split('.')
            .map(|p| p.parse::<u8>().unwrap_or(0));
        (
            it.next().unwrap_or(1),
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
        )
    }

    fn next_header(&mut self) -> Header {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 {
            self.next_id = 1;
        }
        // CONFIG only accepts packets addressed at "me" (127,127); every
        // other class this crate uses accepts that too.
        Header::this_module(id, self.session)
    }

    /// Send one body addressed at the attached module, returning the message
    /// id the device will echo back in its acknowledgement.
    pub fn send(&mut self, body: Body) -> Result<u8> {
        let header = self.next_header();
        let packet = frame::encode(&header, &[body])?;
        if debug_enabled() {
            eprintln!("grid tx: {:?}", String::from_utf8_lossy(&packet));
        }
        self.port.write_all(&packet)?;
        self.port.flush()?;
        Ok(header.id)
    }

    /// Read packets until one carries a `class` body that `accept` approves.
    ///
    /// Non-matching packets are discarded: the module streams heartbeats,
    /// page reports and element events continuously, and a reply to an earlier
    /// request may still be in flight. `accept` must therefore identify the
    /// reply precisely rather than trusting arrival order.
    fn wait_for_body<F>(&mut self, class: u16, accept: F, what: &'static str) -> Result<Frame>
    where
        F: FnMut(&Body) -> Match,
    {
        let deadline = Instant::now() + self.timeout;
        self.wait_for_body_until(class, accept, deadline, what)
    }

    /// As [`Device::wait_for_body`], with an explicit deadline.
    fn wait_for_body_until<F>(
        &mut self,
        class: u16,
        mut accept: F,
        deadline: Instant,
        what: &'static str,
    ) -> Result<Frame>
    where
        F: FnMut(&Body) -> Match,
    {
        let mut chunk = [0u8; 4096];

        loop {
            // Consume whole packets already buffered, oldest first.
            let (packets, rest) = frame::split_packets(&self.pending);
            let rest_len = rest.len();
            let mut matched = None;
            let mut rejected = None;
            for p in &packets {
                if debug_enabled() {
                    eprintln!(
                        "grid rx: {:?} -> {:?}",
                        String::from_utf8_lossy(p),
                        frame::parse(p).map(|f| f
                            .bodies
                            .iter()
                            .map(|b| format!("{}/{:x}", class::name(b.class), b.instr))
                            .collect::<Vec<_>>())
                    );
                }
                // Unparseable packets are skipped; a partial read or foreign
                // traffic must not abort a request.
                let Ok(f) = frame::parse(p) else { continue };
                if let Some(body) = f.body(class) {
                    match accept(body) {
                        Match::Yes => {
                            matched = Some(f);
                            break;
                        }
                        Match::Rejected => rejected = Some(Error::Nak(what)),
                        Match::No => {}
                    }
                }
            }
            // Every complete packet has now been inspected; keep only the tail.
            let total = self.pending.len();
            self.pending.drain(..total - rest_len);

            if let Some(f) = matched {
                return Ok(f);
            }
            if let Some(err) = rejected {
                return Err(err);
            }

            if Instant::now() >= deadline {
                return Err(Error::Timeout {
                    what,
                    ms: self.timeout.as_millis() as u64,
                });
            }

            match self.port.read(&mut chunk) {
                Ok(0) => {}
                Ok(n) => self.pending.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Send `body` and wait for the reply `accept` identifies, re-sending if
    /// the reply does not turn up promptly.
    ///
    /// The module can answer later than a naive single-shot wait allows, and a
    /// late reply left in the buffer would otherwise be picked up by the next
    /// request and silently attributed to it. Re-sending is safe because
    /// `accept` only ever admits the reply belonging to the attempt that is
    /// still outstanding: reads are idempotent, and a repeated write stores
    /// the same bytes again.
    fn request<F>(&mut self, body: Body, mut accept: F, what: &'static str) -> Result<Frame>
    where
        F: FnMut(&Body, &[u8]) -> Match,
    {
        // The module often holds a finished reply until further input
        // arrives, so a prompt re-send is what releases it. Measured on an
        // EN16: re-sending after ~120ms costs about 0.2s per request, while
        // waiting 1.2s before re-sending costs about 2.5s for the same
        // result. Keep this short.
        const ATTEMPT: Duration = Duration::from_millis(120);
        const MAX_ATTEMPTS: usize = 24;
        let class = body.class;
        let overall = Instant::now() + self.timeout;
        // Every id this request has used. A reply to an earlier attempt is
        // still a valid reply to this request, so all of them stay valid.
        let mut ids: Vec<u8> = Vec::new();

        loop {
            if ids.len() >= MAX_ATTEMPTS {
                return Err(Error::Timeout {
                    what,
                    ms: self.timeout.as_millis() as u64,
                });
            }
            self.clear_input();
            ids.push(self.send(body.clone())?);
            let attempt_deadline = (Instant::now() + ATTEMPT).min(overall);

            match self.wait_for_body_until(
                class,
                |reply| accept(reply, &ids),
                attempt_deadline,
                what,
            ) {
                Ok(frame) => return Ok(frame),
                // Out of time overall: report the timeout.
                Err(Error::Timeout { .. }) if Instant::now() < overall => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Send `body` and wait for its acknowledgement.
    ///
    /// The firmware echoes the request id in `LASTHEADER`, so an older
    /// acknowledgement cannot satisfy this.
    fn request_ack(&mut self, body: Body, what: &'static str) -> Result<()> {
        self.request(
            body,
            |reply, ids| {
                let echoed = reply.field_u8(config_fields::LASTHEADER).ok();
                if !echoed.is_some_and(|e| ids.contains(&e)) {
                    return Match::No;
                }
                match reply.instr {
                    instr::ACKNOWLEDGE => Match::Yes,
                    instr::NACKNOWLEDGE => Match::Rejected,
                    _ => Match::No,
                }
            },
            what,
        )?;
        Ok(())
    }

    /// Discard input that arrived before this request was sent.
    ///
    /// This is only an optimisation: replies are matched on their contents, so
    /// anything left behind here is rejected rather than mistaken for the
    /// answer. It therefore must not block - the module streams heartbeats
    /// continuously, and waiting for the port to fall quiet would add a
    /// quarter of a second to every single request.
    fn clear_input(&mut self) {
        self.pending.clear();
        let restore = self.port.timeout();
        if self.port.set_timeout(Duration::from_millis(1)).is_err() {
            return;
        }
        let mut chunk = [0u8; 4096];
        // Bounded so a saturated port cannot spin here.
        for _ in 0..32 {
            match self.port.read(&mut chunk) {
                Ok(n) if n > 0 => {}
                _ => break,
            }
        }
        let _ = self.port.set_timeout(restore);
    }

    /// Read the module's current heartbeat.
    pub fn heartbeat(&mut self) -> Result<Frame> {
        self.wait_for_body(class::HEARTBEAT, |_| Match::Yes, "heartbeat")
    }

    /// Build the fixed part of a CONFIG body.
    fn config_payload(&self, page: u8, element: u8, event: EventType, action: &str) -> String {
        let (major, minor, patch) = self.firmware_triple();
        format!(
            "{major:02x}{minor:02x}{patch:02x}{page:02x}{element:02x}{event:02x}{len:04x}{action}",
            event = event.code(),
            len = action.len(),
        )
    }

    /// Read the Lua action string stored for one (page, element, event).
    pub fn fetch_action(&mut self, page: u8, element: u8, event: EventType) -> Result<String> {
        self.module.element_kind(element)?;
        let payload = self.config_payload(page, element, event, "");

        // The firmware reports the system element as 255 rather than its
        // index, so accept either spelling.
        let system = self.module.system_element();
        let want_element = [element, if element == system { 255 } else { element }];
        let want_event = event.code();

        let frame = self.request(
            Body::new(class::CONFIG, instr::FETCH, payload),
            |body, _ids| {
                if body.instr != instr::REPORT {
                    return Match::No;
                }
                let (Ok(p), Ok(e), Ok(ev)) = (
                    body.field_u8(config_fields::PAGE),
                    body.field_u8(config_fields::ELEMENT),
                    body.field_u8(config_fields::EVENT),
                ) else {
                    return Match::No;
                };
                // Only the reply describing exactly what we asked for will do;
                // a late reply about another element is discarded.
                if p == page && want_element.contains(&e) && ev == want_event {
                    Match::Yes
                } else {
                    Match::No
                }
            },
            "config report",
        )?;

        let body = frame.body(class::CONFIG).expect("matched above");
        let len = body.field(config_fields::ACTION_LENGTH)? as usize;
        let text = body.tail(config_fields::ACTION_STRING)?;
        Ok(if len <= text.len() {
            text[..len].to_string()
        } else {
            text.to_string()
        })
    }

    /// Overwrite the Lua action string for one (page, element, event).
    ///
    /// The change takes effect immediately but lives in RAM until
    /// [`Device::store_page`] is called.
    pub fn write_action(
        &mut self,
        page: u8,
        element: u8,
        event: EventType,
        action: &str,
    ) -> Result<()> {
        self.module.element_kind(element)?;
        if action.len() > ACTIONSTRING_MAXLENGTH {
            return Err(Error::ActionTooLong {
                len: action.len(),
                max: ACTIONSTRING_MAXLENGTH,
            });
        }
        let payload = self.config_payload(page, element, event, action);
        self.request_ack(
            Body::new(class::CONFIG, instr::EXECUTE, payload),
            "config write",
        )
    }

    /// Persist the active page to non-volatile memory.
    pub fn store_page(&mut self) -> Result<()> {
        self.request_ack(
            Body::new(class::PAGESTORE, instr::EXECUTE, ""),
            "page store",
        )
    }

    /// Discard unsaved changes to the active page, reloading it from memory.
    pub fn discard_page(&mut self) -> Result<()> {
        self.request_ack(
            Body::new(class::PAGEDISCARD, instr::EXECUTE, ""),
            "page discard",
        )
    }

    /// Reset the active page to the firmware defaults (in RAM).
    pub fn clear_page(&mut self) -> Result<()> {
        self.request_ack(
            Body::new(class::PAGECLEAR, instr::EXECUTE, ""),
            "page clear",
        )
    }

    /// Which page the module is currently on.
    pub fn active_page(&mut self) -> Result<u8> {
        let frame = self.wait_for_body(class::PAGEACTIVE, |_| Match::Yes, "active page")?;
        let body = frame.body(class::PAGEACTIVE).expect("matched above");
        body.field_u8((5, 2))
    }

    /// Run a Lua snippet on the module right now, without storing anything.
    ///
    /// The script is framed to match the running firmware. Up to and
    /// including 1.5.1 the handler refuses anything that is not wrapped:
    ///
    /// ```c
    /// if (strncmp(script, "<?lua ", 6) != 0) { return 1; }
    /// if (strncmp(&script[length - 3], " ?>", 3) != 0) { return 1; }
    /// ```
    ///
    /// From 1.5.2 on the wrapper check is gone and the handler executes the
    /// bare string, where a wrapper would instead be a Lua syntax error. So
    /// the framing has to follow the version, which is why sending unwrapped
    /// Lua to a 1.5.1 module silently did nothing.
    ///
    /// Firmware before 1.5.0 has no immediate handler at all and this call
    /// is rejected rather than sent into the void.
    pub fn immediate(&mut self, lua: &str) -> Result<()> {
        if !self.supports(Feature::Immediate) {
            return Err(Error::UnsupportedFirmware {
                feature: "running a script immediately",
                found: self.info.firmware.clone(),
                needs: Feature::Immediate.since_str(),
            });
        }
        let framed = if Self::immediate_needs_wrapper(self.firmware_triple()) {
            format!("<?lua {lua} ?>")
        } else {
            lua.to_string()
        };
        if framed.len() > ACTIONSTRING_MAXLENGTH {
            return Err(Error::ActionTooLong {
                len: framed.len(),
                max: ACTIONSTRING_MAXLENGTH,
            });
        }
        let payload = format!("{len:04x}{framed}", len = framed.len());
        self.send(Body::new(class::IMMEDIATE, instr::EXECUTE, payload))?;
        Ok(())
    }

    /// Whether this firmware's immediate handler demands the `<?lua ... ?>`
    /// wrapper. True for 1.5.0 and 1.5.1, false from 1.5.2 on.
    pub(crate) fn immediate_needs_wrapper(version: (u8, u8, u8)) -> bool {
        version < (1, 5, 2)
    }

    /// Whether the running firmware provides `feature`.
    pub fn supports(&self, feature: Feature) -> bool {
        self.firmware_triple() >= feature.since()
    }

    /// The running firmware version as `(major, minor, patch)`.
    pub fn firmware_version(&self) -> (u8, u8, u8) {
        self.firmware_triple()
    }

    /// Reject a call whose feature the running firmware lacks.
    fn require(&self, feature: Feature) -> Result<()> {
        if self.supports(feature) {
            return Ok(());
        }
        Err(Error::UnsupportedFirmware {
            feature: feature.as_str(),
            found: self.info.firmware.clone(),
            needs: feature.since_str(),
        })
    }

    /// Reboot the module.
    pub fn reset(&mut self) -> Result<()> {
        self.send(Body::new(class::RESET, instr::EXECUTE, ""))?;
        Ok(())
    }

    /// The event whose script holds an element's LED and value settings.
    fn value_event(&self, element: u8) -> Result<(ElementKind, EventType)> {
        let kind = self.module.element_kind(element)?;
        let event = kind.value_event().ok_or(Error::Unsupported {
            module: self.module.kind.as_str(),
            element,
            kind: kind.as_str(),
            what: "value or LED settings",
        })?;
        Ok((kind, event))
    }

    /// Read an element's stored settings as a parsed action string.
    pub fn element_action(&mut self, page: u8, element: u8) -> Result<(EventType, ActionString)> {
        let (_, event) = self.value_event(element)?;
        let raw = self.fetch_action(page, element, event)?;
        Ok((event, ActionString::parse(&raw)))
    }

    /// Set an element's LED colour persistently (RAM until `store_page`).
    pub fn set_led_color(&mut self, page: u8, element: u8, color: Rgb, alpha: f32) -> Result<()> {
        let (kind, event) = self.value_event(element)?;
        if !kind.has_led() {
            return Err(Error::Unsupported {
                module: self.module.kind.as_str(),
                element,
                kind: kind.as_str(),
                what: "LED",
            });
        }
        let raw = self.fetch_action(page, element, event)?;
        let mut action = ActionString::parse(&raw);
        action.set_led_color(color, alpha);
        let rendered = action.render_checked()?;
        if rendered == raw {
            return Ok(());
        }
        self.write_action(page, element, event, &rendered)
    }

    /// Reset an element's LED colour to the module default.
    pub fn clear_led_color(&mut self, page: u8, element: u8) -> Result<()> {
        let (_, event) = self.value_event(element)?;
        let raw = self.fetch_action(page, element, event)?;
        let mut action = ActionString::parse(&raw);
        action.clear_led_color();
        let rendered = action.render_checked()?;
        if rendered == raw {
            return Ok(());
        }
        self.write_action(page, element, event, &rendered)
    }

    /// Read an element's stored LED colour, if it is set to a literal.
    ///
    /// Checks the element's value event first, then its init script, since
    /// older firmware keeps the colour in init.
    pub fn led_color(&mut self, page: u8, element: u8) -> Result<Option<Rgb>> {
        let (_, action) = self.element_action(page, element)?;
        if let Some(c) = action.led_color().or_else(|| action.global_led_color()) {
            return Ok(Some(c));
        }
        let init = ActionString::parse(&self.fetch_action(page, element, EventType::Init)?);
        Ok(init.led_color().or_else(|| init.global_led_color()))
    }

    /// Light an element's LED right now, without changing stored config.
    ///
    /// The colour is written to `layer`, defaulting to the layer the element's
    /// own event handler uses, and is overwritten again as soon as the element
    /// is touched.
    pub fn flash_led(&mut self, element: u8, color: Rgb, layer_override: Option<u8>) -> Result<()> {
        let (kind, event) = self.value_event(element)?;
        if !kind.has_led() {
            return Err(Error::Unsupported {
                module: self.module.kind.as_str(),
                element,
                kind: kind.as_str(),
                what: "LED",
            });
        }
        let l = layer_override
            .or_else(|| event.auto_layer())
            .unwrap_or(layer::UI_A);
        self.immediate(&format!(
            "ele[{element}]:glc({l},{{{{{r},{g},{b},1}}}})",
            r = color.r,
            g = color.g,
            b = color.b
        ))
    }

    /// Set an element's LED brightness/value right now (`0..=255`).
    pub fn flash_led_value(
        &mut self,
        element: u8,
        value: u8,
        layer_override: Option<u8>,
    ) -> Result<()> {
        let (_, event) = self.value_event(element)?;
        let l = layer_override
            .or_else(|| event.auto_layer())
            .unwrap_or(layer::UI_A);
        self.immediate(&format!("ele[{element}]:glp({l},{value})"))
    }

    /// The action block that holds the settings for an element kind.
    fn settings_block(kind: ElementKind) -> &'static str {
        match kind {
            ElementKind::Encoder => ENCODER_BLOCK,
            ElementKind::Button => BUTTON_BLOCK,
            ElementKind::Potmeter => POTMETER_BLOCK,
            _ => ENCODER_BLOCK,
        }
    }

    /// Change one `self:<name>(<value>)` setting on an element.
    pub fn set_element_setting(
        &mut self,
        page: u8,
        element: u8,
        name: &str,
        value: i64,
    ) -> Result<()> {
        let (kind, event) = self.value_event(element)?;
        let raw = self.fetch_action(page, element, event)?;
        let mut action = ActionString::parse(&raw);
        action.set_setting(Self::settings_block(kind), name, value);
        let rendered = action.render_checked()?;
        if rendered == raw {
            return Ok(());
        }
        self.write_action(page, element, event, &rendered)
    }

    /// Turn an element's LED off persistently (RAM until `store_page`).
    ///
    /// This zeroes the colour in *every* event script the element has, not
    /// just the one that reacts to turning it. An encoder also has a button
    /// event with its own colour block, so setting only one leaves the LED
    /// lighting up again as soon as the element is pressed.
    /// Returns how many scripts were actually rewritten.
    pub fn set_led_off(&mut self, page: u8, element: u8) -> Result<usize> {
        self.set_led_color_everywhere(page, element, Rgb::BLACK)
    }

    /// Write `color` into every script of an element that drives its LED,
    /// whichever dialect that script uses.
    ///
    /// Returns how many scripts were rewritten.
    pub fn set_led_color_everywhere(&mut self, page: u8, element: u8, color: Rgb) -> Result<usize> {
        use crate::action::LedDialect;

        let kind = self.module.element_kind(element)?;
        if !kind.has_led() {
            return Err(Error::Unsupported {
                module: self.module.kind.as_str(),
                element,
                kind: kind.as_str(),
                what: "LED",
            });
        }

        let mut written = 0;
        for &event in kind.events() {
            let raw = self.fetch_action(page, element, event)?;
            let mut action = ActionString::parse(&raw);

            // Firmware versions store the colour differently, so follow
            // whichever form this script already uses and leave scripts that
            // do not drive the LED alone.
            match action.led_dialect() {
                LedDialect::Curve => action.set_led_color(color, 1.0),
                LedDialect::Global => {
                    if action.set_global_led_color(color) == 0 {
                        continue;
                    }
                }
                LedDialect::None => continue,
            }

            let rendered = action.render_checked()?;
            if rendered != raw {
                self.write_action(page, element, event, &rendered)?;
                written += 1;
            }
        }
        Ok(written)
    }

    /// Turn an element's LED off right now, without changing stored config.
    ///
    /// Writes black to every layer, so nothing underneath shows through.
    pub fn flash_led_off(&mut self, element: u8) -> Result<()> {
        let kind = self.module.element_kind(element)?;
        if !kind.has_led() {
            return Err(Error::Unsupported {
                module: self.module.kind.as_str(),
                element,
                kind: kind.as_str(),
                what: "LED",
            });
        }
        let mut lua = String::new();
        for l in 0..layer::COUNT {
            lua.push_str(&format!("ele[{element}]:glc({l},{{{{0,0,0,1}}}})"));
            lua.push_str(&format!("ele[{element}]:glp({l},0)"));
        }
        self.immediate(&lua)
    }

    /// Assign an element's value from software, right now.
    ///
    /// The value is written to the element's own value variable, so the next
    /// physical move continues from it rather than from where it was, and the
    /// LED is updated to match by mapping the value across the element's
    /// configured min/max range - the same mapping the firmware's own event
    /// handlers use.
    ///
    /// Requires the element to be in absolute mode (`mode = 0`), which is the
    /// firmware default; in relative mode the hardware reports deltas and the
    /// stored value is not a position.
    pub fn set_element_value(&mut self, element: u8, value: u8) -> Result<()> {
        let (kind, event) = self.value_event(element)?;
        let p = kind.value_prefix().ok_or(Error::Unsupported {
            module: self.module.kind.as_str(),
            element,
            kind: kind.as_str(),
            what: "assignable value",
        })?;
        let l = event.auto_layer().unwrap_or(layer::UI_A);

        // Everything is computed on the module so no range read is needed and
        // the LED cannot disagree with the value that was just set.
        self.immediate(&format!(
            "local e=ele[{element}] e:{p}va({value}) e:glp({l},gmaps(e:{p}va(),e:{p}mi(),e:{p}ma(),0,255)//1)"
        ))
    }

    /// As [`Device::set_element_value`], but as a percentage of the element's
    /// own min/max range, which the module resolves itself.
    pub fn set_element_percent(&mut self, element: u8, percent: f32) -> Result<()> {
        let (kind, event) = self.value_event(element)?;
        let p = kind.value_prefix().ok_or(Error::Unsupported {
            module: self.module.kind.as_str(),
            element,
            kind: kind.as_str(),
            what: "assignable value",
        })?;
        let l = event.auto_layer().unwrap_or(layer::UI_A);
        let pct = percent.clamp(0.0, 100.0);
        // Scaled by 1000 and divided on the module, to keep the action string
        // free of locale-dependent float formatting.
        let milli = (pct * 10.0).round() as i64;

        self.immediate(&format!(
            "local e=ele[{element}] local a=e:{p}mi() local b=e:{p}ma() \
             local v=a+((b-a)*{milli})//1000 e:{p}va(v) \
             e:glp({l},gmaps(v,a,b,0,255)//1)"
        ))
    }

    /// Which MIDI message an element should follow, and what it should do.
    pub fn bind_midi_rx(&mut self, page: u8, element: u8, binding: &MidiRxBinding) -> Result<()> {
        // midirx_register does not exist before 1.5.2, so writing this
        // block to an older module would store Lua that raises an error at
        // init instead of binding anything.
        self.require(Feature::MidiRxBinding)?;
        let kind = self.module.element_kind(element)?;
        let p = kind.value_prefix().ok_or(Error::Unsupported {
            module: self.module.kind.as_str(),
            element,
            kind: kind.as_str(),
            what: "MIDI receive binding",
        })?;

        // The binding must be installed by the element's init script: that is
        // what runs on boot and on page load, and `midirx_register` only
        // stores a callback on the element.
        let raw = self.fetch_action(page, element, EventType::Init)?;
        let mut action = ActionString::parse(&raw);
        action.set_block(crate::action::MIDIRX_BLOCK, binding.to_lua(p));
        let rendered = action.render_checked()?;
        if rendered != raw {
            self.write_action(page, element, EventType::Init, &rendered)?;
        }
        Ok(())
    }

    /// Remove a MIDI receive binding written by [`Device::bind_midi_rx`].
    pub fn unbind_midi_rx(&mut self, page: u8, element: u8) -> Result<bool> {
        let raw = self.fetch_action(page, element, EventType::Init)?;
        let mut action = ActionString::parse(&raw);
        if !action.remove_block(crate::action::MIDIRX_BLOCK) {
            return Ok(false);
        }
        let rendered = action.render_checked()?;
        self.write_action(page, element, EventType::Init, &rendered)?;
        Ok(true)
    }

    /// Read the MIDI receive binding stored on an element, if any.
    pub fn midi_rx_binding(&mut self, page: u8, element: u8) -> Result<Option<String>> {
        let raw = self.fetch_action(page, element, EventType::Init)?;
        Ok(ActionString::parse(&raw)
            .block(crate::action::MIDIRX_BLOCK)
            .map(|b| b.body.clone()))
    }

    /// Set how incoming MIDI of `rx_type` is routed, right now.
    ///
    /// `mode` is a bitmask: `0x01` forward-from-USB, `0x02` handle-external,
    /// `0x04` handle-internal (the firmware's `grid_rx_features`). A module
    /// only acts on MIDI itself when handle-internal is set.
    pub fn set_rx_mode(&mut self, rx_type: u8, mode: u8) -> Result<()> {
        self.require(Feature::RxMode)?;
        self.immediate(&format!("rx_mode({rx_type},{mode})"))
    }

    /// Persist an `rx_mode` call in the module's system init script, so the
    /// routing survives a power cycle.
    pub fn persist_rx_mode(&mut self, page: u8, rx_type: u8, mode: u8) -> Result<()> {
        self.require(Feature::RxMode)?;
        let system = self.module.system_element();
        let raw = self.fetch_action(page, system, EventType::Init)?;
        let mut action = ActionString::parse(&raw);
        action.set_block("grxm", format!("rx_mode({rx_type},{mode})"));
        let rendered = action.render_checked()?;
        if rendered != raw {
            self.write_action(page, system, EventType::Init, &rendered)?;
        }
        Ok(())
    }

    /// The short firmware setting names for an element kind, as
    /// (friendly, firmware) pairs.
    ///
    /// These are the `self:<name>(..)` accessors the firmware's own default
    /// action strings use, keyed by the element kind's value prefix.
    pub fn setting_names(kind: ElementKind) -> &'static [(&'static str, &'static str)] {
        match kind {
            ElementKind::Encoder => &[
                ("mode", "emo"),
                ("min", "emi"),
                ("max", "ema"),
                ("sensitivity", "ese"),
                ("velocity", "ev0"),
            ],
            ElementKind::Endless => &[
                ("mode", "epmo"),
                ("min", "epmi"),
                ("max", "epma"),
                ("sensitivity", "epse"),
                ("velocity", "epv0"),
            ],
            ElementKind::Potmeter => &[("mode", "pmo"), ("min", "pmi"), ("max", "pma")],
            ElementKind::Button => &[("mode", "bmo"), ("min", "bmi"), ("max", "bma")],
            _ => &[],
        }
    }

    /// Read an element's stored behaviour settings.
    ///
    /// This is the read side of [`Device::set_element_setting`] and
    /// [`Device::set_knob_mode`]: it parses the element's own script rather
    /// than assuming defaults, so a caller can decide whether a change is
    /// even needed. Fields the script does not set come back as `None`,
    /// which is how the firmware expresses "use the element default".
    pub fn element_settings(&mut self, page: u8, element: u8) -> Result<ElementSettings> {
        let kind = self.module.element_kind(element)?;
        let (_, action) = self.element_action(page, element)?;

        let get = |name: &str| -> Option<i64> {
            Self::setting_names(kind)
                .iter()
                .find(|(friendly, _)| *friendly == name)
                .and_then(|(_, short)| action.setting(short))
        };

        let mode_raw = get("mode");
        Ok(ElementSettings {
            mode: mode_raw.and_then(KnobMode::from_code),
            mode_raw,
            min: get("min"),
            max: get("max"),
            sensitivity: get("sensitivity"),
            velocity: get("velocity"),
        })
    }

    /// Set how an element turns movement into a value (absolute, relative or
    /// signed relative), persistently.
    ///
    /// Only kinds that accumulate a value have a mode; a button or system
    /// element is rejected rather than silently ignored.
    pub fn set_knob_mode(&mut self, page: u8, element: u8, mode: KnobMode) -> Result<()> {
        let kind = self.module.element_kind(element)?;
        if !matches!(kind, ElementKind::Encoder | ElementKind::Endless) {
            return Err(Error::Unsupported {
                module: self.module.kind.as_str(),
                element,
                kind: kind.as_str(),
                what: "a movement mode (only encoders and endless potentiometers have one)",
            });
        }
        let short = Self::setting_names(kind)
            .iter()
            .find(|(f, _)| *f == "mode")
            .map(|(_, s)| *s)
            .expect("encoder and endless both define a mode setting");
        self.set_element_setting(page, element, short, mode.code())
    }

    /// Set an element's LED animation, persistently.
    ///
    /// Written as the firmware's global `glf` (rate) and `gls` (type) calls
    /// addressed by `self:ind()`, which both firmware families expose, so
    /// this works regardless of which colour dialect the module stores.
    /// [`LedAnimation::STEADY`] restores the firmware's cleared state.
    pub fn set_led_animation(
        &mut self,
        page: u8,
        element: u8,
        animation: LedAnimation,
        layer_override: Option<u8>,
    ) -> Result<()> {
        let (kind, event) = self.value_event(element)?;
        if !kind.has_led() {
            return Err(Error::Unsupported {
                module: self.module.kind.as_str(),
                element,
                kind: kind.as_str(),
                what: "LED",
            });
        }
        let l = layer_override
            .or_else(|| event.auto_layer())
            .unwrap_or(layer::UI_A);

        let raw = self.fetch_action(page, element, event)?;
        let mut action = ActionString::parse(&raw);
        action.set_block(
            ANIMATION_BLOCK,
            format!(
                "glf(self:ind(),{l},{rate})gls(self:ind(),{l},{shape})",
                rate = animation.rate,
                shape = animation.shape
            ),
        );
        let rendered = action.render_checked()?;
        if rendered == raw {
            return Ok(());
        }
        self.write_action(page, element, event, &rendered)
    }

    /// Remove an LED animation block written by [`Device::set_led_animation`].
    pub fn clear_led_animation(&mut self, page: u8, element: u8) -> Result<bool> {
        let (_, event) = self.value_event(element)?;
        let raw = self.fetch_action(page, element, event)?;
        let mut action = ActionString::parse(&raw);
        if !action.remove_block(ANIMATION_BLOCK) {
            return Ok(false);
        }
        let rendered = action.render_checked()?;
        self.write_action(page, element, event, &rendered)?;
        Ok(true)
    }

    /// Drive an element's LED animation right now, without storing it.
    pub fn flash_led_animation(
        &mut self,
        element: u8,
        animation: LedAnimation,
        layer_override: Option<u8>,
    ) -> Result<()> {
        let (_, event) = self.value_event(element)?;
        let l = layer_override
            .or_else(|| event.auto_layer())
            .unwrap_or(layer::UI_A);
        self.immediate(&format!(
            "glf({element},{l},{rate})gls({element},{l},{shape})",
            rate = animation.rate,
            shape = animation.shape
        ))
    }
}

/// A firmware capability, with the release that introduced it.
///
/// Grid firmware gained features over time, and a module silently ignores
/// Lua it does not know: calling a missing global raises an error inside the
/// script, which stores or runs but does nothing useful. Checking first turns
/// that into an explicit refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `IMMEDIATE` class handling, i.e. running a script right now.
    /// `grid_decode.c` has no immediate handler before 1.5.0.
    Immediate,
    /// `rx_mode(..)`, for routing incoming MIDI. Added in 1.5.2.
    RxMode,
    /// `midirx_register(..)` / `self:gmrr(..)` bindings. Added in 1.5.2.
    MidiRxBinding,
}

impl Feature {
    /// First firmware version that provides this feature.
    pub fn since(self) -> (u8, u8, u8) {
        match self {
            Feature::Immediate => (1, 5, 0),
            Feature::RxMode | Feature::MidiRxBinding => (1, 5, 2),
        }
    }

    /// The same version, for messages.
    pub fn since_str(self) -> &'static str {
        match self {
            Feature::Immediate => "1.5.0",
            Feature::RxMode | Feature::MidiRxBinding => "1.5.2",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Feature::Immediate => "running a script immediately",
            Feature::RxMode => "MIDI receive routing (rx_mode)",
            Feature::MidiRxBinding => "MIDI receive bindings (midirx_register)",
        }
    }
}

/// A request to make an element follow an incoming MIDI message.
///
/// `None` means "the firmware's automatic value", which is what the Grid
/// editor's `-1` placeholders mean: channel from module position and page,
/// command from the event kind, and CC from the element index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MidiRxBinding {
    pub channel: Option<u8>,
    pub command: Option<u8>,
    pub parameter: Option<u8>,
    /// Move the element's value to the received MIDI value.
    pub set_value: bool,
    /// Scale the LED brightness with the received MIDI value.
    pub set_led: bool,
}

impl MidiRxBinding {
    /// Follow MIDI with the LED brightness only, leaving the value alone.
    pub fn led_only() -> Self {
        MidiRxBinding {
            set_led: true,
            ..Default::default()
        }
    }

    /// Follow MIDI with both the value and the LED, on automatic addressing.
    pub fn automatic() -> Self {
        MidiRxBinding {
            set_value: true,
            set_led: true,
            ..Default::default()
        }
    }

    /// Render the `self:gmrr(..)` call that installs this binding.
    ///
    /// `prefix` is the element kind's value prefix: the firmware keys the
    /// callback's value and range lookups on it (`eva`/`emi`/`ema`), and its
    /// own `-1` default would resolve to the *init* handler's name here,
    /// which is why it is always written explicitly.
    pub fn to_lua(&self, prefix: &str) -> String {
        fn arg(v: Option<u8>) -> String {
            match v {
                Some(v) => v.to_string(),
                None => "-1".to_string(),
            }
        }
        format!(
            "self:gmrr('{prefix}',{ch},{cmd},{p1},{{{set_value},{set_led}}})",
            ch = arg(self.channel),
            cmd = arg(self.command),
            p1 = arg(self.parameter),
            set_value = self.set_value,
            set_led = self.set_led,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_wrapping_follows_the_firmware_version() {
        // 1.5.1's handler rejects anything not wrapped in `<?lua ... ?>`;
        // 1.5.2 dropped the check and runs the bare string, where a wrapper
        // would be a syntax error. Getting this backwards is why unwrapped
        // scripts silently did nothing on a 1.5.1 EN16.
        assert!(Device::immediate_needs_wrapper((1, 5, 0)));
        assert!(Device::immediate_needs_wrapper((1, 5, 1)));
        assert!(!Device::immediate_needs_wrapper((1, 5, 2)));
        assert!(!Device::immediate_needs_wrapper((1, 5, 6)));
        // Older lines never reach the wrapper question: they have no handler.
        assert!(Device::immediate_needs_wrapper((1, 2, 35)));
    }

    #[test]
    fn feature_gates_match_the_releases_that_added_them() {
        assert_eq!(Feature::Immediate.since(), (1, 5, 0));
        assert_eq!(Feature::RxMode.since(), (1, 5, 2));
        assert_eq!(Feature::MidiRxBinding.since(), (1, 5, 2));

        // The two modules on this bench, against each feature.
        let en16 = (1u8, 5u8, 1u8); // EN16 RevD
        let ef44 = (1u8, 2u8, 35u8); // EF44 RevD
        assert!(en16 >= Feature::Immediate.since(), "EN16 has an immediate handler");
        assert!(en16 < Feature::RxMode.since(), "EN16 predates rx_mode");
        assert!(en16 < Feature::MidiRxBinding.since(), "EN16 predates midirx_register");
        assert!(ef44 < Feature::Immediate.since(), "EF44 has no immediate handler");
        assert!(ef44 < Feature::MidiRxBinding.since());
    }

    #[test]
    fn automatic_binding_matches_the_firmware_signature() {
        // midirx_register(event, channel, command, parameter1, features)
        // with features = { set_value, set_led }; -1 means "automatic".
        let b = MidiRxBinding::automatic();
        assert_eq!(b.to_lua("e"), "self:gmrr('e',-1,-1,-1,{true,true})");
    }

    #[test]
    fn led_only_binding_leaves_the_value_alone() {
        assert_eq!(
            MidiRxBinding::led_only().to_lua("e"),
            "self:gmrr('e',-1,-1,-1,{false,true})"
        );
    }

    #[test]
    fn explicit_addressing_is_rendered_literally() {
        let b = MidiRxBinding {
            channel: Some(0),
            command: Some(0xB0),
            parameter: Some(20),
            set_value: true,
            set_led: true,
        };
        assert_eq!(b.to_lua("e"), "self:gmrr('e',0,176,20,{true,true})");
    }

    #[test]
    fn prefix_comes_from_the_element_kind() {
        // The callback looks up `<prefix>va`, `<prefix>mi` and `<prefix>ma`,
        // so the prefix must match the element kind, not the event name.
        assert_eq!(ElementKind::Encoder.value_prefix(), Some("e"));
        assert_eq!(ElementKind::Button.value_prefix(), Some("b"));
        assert_eq!(ElementKind::Potmeter.value_prefix(), Some("p"));
        assert_eq!(ElementKind::Endless.value_prefix(), Some("ep"));
        assert_eq!(ElementKind::System.value_prefix(), None);

        let b = MidiRxBinding::automatic();
        assert!(b.to_lua("p").starts_with("self:gmrr('p',"));
    }
}
