//! Reading and editing the Lua action strings the device stores per event.
//!
//! The firmware stores one string per (page, element, event). The Grid editor
//! splits it into "action blocks", each introduced by a `--[[@id]]` comment,
//! and the firmware's own defaults use the same shape, e.g. the default
//! encoder event script:
//!
//! ```text
//! --[[@sec]]self:emo(0)self:ev0(50)self:emi(0)self:ema(127)self:ese(100)
//! --[[@sglc]]self:glc(-1,{{-1,-1,-1,1}})self:glp(-1,-1)
//! --[[@gms]]self:gms(-1,-1,-1,-1)
//! ```
//!
//! Editing a single setting therefore means rewriting one call inside one
//! block and leaving everything else byte-identical, which is what this module
//! does. `-1` means "use the element default" throughout.

use crate::color::Rgb;
use crate::error::{Error, Result};
use crate::proto::ACTIONSTRING_MAXLENGTH;

/// The action block that carries LED colour and value, as named by the editor.
pub const LED_BLOCK: &str = "sglc";
/// The action block that carries encoder settings.
pub const ENCODER_BLOCK: &str = "sec";
/// The action block that carries button settings.
pub const BUTTON_BLOCK: &str = "sbc";
/// The action block that carries potmeter settings.
pub const POTMETER_BLOCK: &str = "spc";
/// The action block that carries the MIDI message.
pub const MIDI_BLOCK: &str = "gms";
/// The action block this crate uses for the MIDI receive binding.
pub const MIDIRX_BLOCK: &str = "gmrr";
/// The action block this crate uses for LED animation parameters.
pub const ANIMATION_BLOCK: &str = "glanim";

/// One `--[[@id]]...` block of an action string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Block id, without the `--[[@` and `]]` delimiters.
    pub id: String,
    /// Block contents, excluding the introducing comment.
    pub body: String,
}

impl Block {
    /// Render back into `--[[@id]]body`.
    pub fn render(&self) -> String {
        format!("--[[@{}]]{}", self.id, self.body)
    }
}

/// A parsed action string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionString {
    /// Anything before the first block marker, e.g. the `<?lua ` the device
    /// wraps stored scripts in.
    pub preamble: String,
    pub blocks: Vec<Block>,
    /// The trailing `?>` terminator, kept separate so that blocks appended to
    /// the script land *inside* the Lua region rather than after its end.
    pub epilogue: String,
}

/// The prefix firmware up to 1.5.1 stores action strings with.
///
/// `GRID_ACTION_PREFIX` in `grid_ui.h`.
pub const ACTION_PREFIX: &str = "<?lua ";
/// The matching suffix, `GRID_ACTION_SUFFIX`.
pub const ACTION_SUFFIX: &str = " ?>";

/// Re-frame a stored action string for the firmware it is being written to.
///
/// Firmware up to 1.5.1 stores actions wrapped in `<?lua ... ?>` - its own
/// defaults are written that way, e.g.
/// `GRID_ACTIONSTRING_ENCODER_INIT "<?lua --[[@cb]] --[[Encoder Init]] ?>"`.
/// From 1.5.2 the defaults are bare: `"--[[@cb]]--[[Encoder Init]]"`.
///
/// This matters when a configuration dumped from one firmware is loaded onto
/// another: writing a wrapped script to a module whose convention is bare, or
/// the reverse, stores a script the firmware will not execute as intended.
/// Returns `text` unchanged when it already matches `wrapped`.
pub fn rewrap(text: &str, wrapped: bool) -> String {
    let bare = text
        .strip_prefix(ACTION_PREFIX)
        .map(|head| head.strip_suffix(ACTION_SUFFIX).unwrap_or(head))
        .unwrap_or(text);

    if wrapped {
        format!("{ACTION_PREFIX}{bare}{ACTION_SUFFIX}")
    } else {
        bare.to_string()
    }
}

impl ActionString {
    /// Parse an action string into its blocks.
    ///
    /// Strings without any marker are kept verbatim in `preamble`, so that
    /// round-tripping never damages a hand-written script.
    pub fn parse(text: &str) -> Self {
        const MARK: &str = "--[[@";
        let mut blocks = Vec::new();

        let first = match text.find(MARK) {
            Some(i) => i,
            None => {
                return ActionString {
                    preamble: text.to_string(),
                    blocks,
                    epilogue: String::new(),
                }
            }
        };
        let preamble = text[..first].to_string();

        // Split off a trailing `?>` (with the whitespace in front of it) so
        // that appending a block cannot push it outside the Lua region, which
        // the firmware would then ignore.
        let (text, epilogue) = match text.strip_suffix("?>") {
            Some(head) => {
                let trimmed = head.trim_end_matches([' ', '\t', '\n', '\r']);
                (trimmed, text[trimmed.len()..].to_string())
            }
            None => (text, String::new()),
        };

        let mut cursor = first;
        while cursor < text.len() {
            // cursor points at a MARK.
            let after_mark = cursor + MARK.len();
            let close = match text[after_mark..].find("]]") {
                Some(i) => after_mark + i,
                None => {
                    // Unterminated marker: keep the remainder as a body-less block.
                    blocks.push(Block {
                        id: String::new(),
                        body: text[cursor..].to_string(),
                    });
                    break;
                }
            };
            let id = text[after_mark..close].to_string();
            let body_start = close + 2;
            let next = text[body_start..]
                .find(MARK)
                .map(|i| body_start + i)
                .unwrap_or(text.len());
            blocks.push(Block {
                id,
                body: text[body_start..next].to_string(),
            });
            cursor = next;
        }

        ActionString {
            preamble,
            blocks,
            epilogue,
        }
    }

    /// Render back to the exact string the device stores.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.preamble.len() + 32 * self.blocks.len());
        out.push_str(&self.preamble);
        for b in &self.blocks {
            out.push_str(&b.render());
        }
        out.push_str(&self.epilogue);
        out
    }

    /// Render and check against the firmware's length limit.
    pub fn render_checked(&self) -> Result<String> {
        let s = self.render();
        if s.len() > ACTIONSTRING_MAXLENGTH {
            return Err(Error::ActionTooLong {
                len: s.len(),
                max: ACTIONSTRING_MAXLENGTH,
            });
        }
        Ok(s)
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn block_mut(&mut self, id: &str) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    /// Replace a block's body, appending the block if it does not exist.
    pub fn set_block(&mut self, id: &str, body: impl Into<String>) {
        let body = body.into();
        match self.block_mut(id) {
            Some(b) => b.body = body,
            None => self.blocks.push(Block {
                id: id.to_string(),
                body,
            }),
        }
    }

    /// Remove a block, returning whether it was there.
    pub fn remove_block(&mut self, id: &str) -> bool {
        let before = self.blocks.len();
        self.blocks.retain(|b| b.id != id);
        self.blocks.len() != before
    }

    /// Which LED colour dialect this script uses.
    pub fn led_dialect(&self) -> LedDialect {
        if self.block(LED_BLOCK).is_some()
            || self.blocks.iter().any(|b| b.body.contains("self:glc("))
        {
            return LedDialect::Curve;
        }
        if self
            .blocks
            .iter()
            .any(|b| !global_glc_arg_ranges(&b.body).is_empty())
        {
            return LedDialect::Global;
        }
        LedDialect::None
    }

    /// Rewrite the colour of every global `glc(num, layer, r, g, b)` call,
    /// keeping each call's element and layer arguments.
    ///
    /// Returns how many calls were changed. Used by firmware 1.2.x configs,
    /// which set the colour from `glr()`/`glg()`/`glb()` locals; replacing the
    /// arguments with literals leaves those locals unused but harmless.
    pub fn set_global_led_color(&mut self, color: Rgb) -> usize {
        let mut changed = 0;
        for block in &mut self.blocks {
            // Rewrite back to front so earlier ranges stay valid.
            let ranges = global_glc_arg_ranges(&block.body);
            for range in ranges.into_iter().rev() {
                let args: Vec<&str> = block.body[range.clone()].split(',').collect();
                if args.len() < 5 {
                    continue;
                }
                let replacement = format!(
                    "{},{},{},{},{}",
                    args[0].trim(),
                    args[1].trim(),
                    color.r,
                    color.g,
                    color.b
                );
                block.body.replace_range(range, &replacement);
                changed += 1;
            }
        }
        changed
    }

    /// Read the colour out of the first global `glc(..)` call, if it is a
    /// literal.
    pub fn global_led_color(&self) -> Option<Rgb> {
        for block in &self.blocks {
            for range in global_glc_arg_ranges(&block.body) {
                let args: Vec<&str> = block.body[range].split(',').collect();
                if args.len() < 5 {
                    continue;
                }
                let parsed: Option<Vec<u8>> = args[2..5]
                    .iter()
                    .map(|a| a.trim().parse::<u8>().ok())
                    .collect();
                if let Some(v) = parsed {
                    return Some(Rgb::new(v[0], v[1], v[2]));
                }
            }
        }
        None
    }

    /// Read the colour currently set by the `sglc` block, if it is a literal.
    ///
    /// Returns `None` when the block is missing or uses `-1` defaults.
    pub fn led_color(&self) -> Option<Rgb> {
        let body = &self.block(LED_BLOCK)?.body;
        let curve = parse_glc_curve(body)?;
        let last = curve.last()?;
        let (r, g, b) = (last[0], last[1], last[2]);
        if r < 0 || g < 0 || b < 0 {
            return None;
        }
        Some(Rgb::new(
            r.clamp(0, 255) as u8,
            g.clamp(0, 255) as u8,
            b.clamp(0, 255) as u8,
        ))
    }

    /// Set the LED colour in the `sglc` block, preserving the `glp` call.
    ///
    /// `alpha` scales the colour on the device (`0.0..=1.0`); the firmware
    /// multiplies the channels by it.
    pub fn set_led_color(&mut self, color: Rgb, alpha: f32) {
        let a = format_alpha(alpha);
        let glc = format!(
            "self:glc(-1,{{{{{r},{g},{b},{a}}}}})",
            r = color.r,
            g = color.g,
            b = color.b,
            a = a
        );
        match self.block_mut(LED_BLOCK) {
            Some(block) => match call_range(&block.body, "self:glc") {
                // Replace in place so surrounding whitespace and any
                // `<?lua ?>` wrapper stay byte-identical.
                Some(range) => block.body.replace_range(range, &glc),
                None => block.body.push_str(&glc),
            },
            None => {
                // Keep the firmware's companion call so the LED still follows the value.
                self.set_block(LED_BLOCK, format!("{glc}self:glp(-1,-1)"));
            }
        }
    }

    /// Reset the LED colour block to the firmware default (`-1` everywhere).
    pub fn clear_led_color(&mut self) {
        let default = "self:glc(-1,{{-1,-1,-1,1}})".to_string();
        if let Some(block) = self.block_mut(LED_BLOCK) {
            match call_range(&block.body, "self:glc") {
                Some(range) => block.body.replace_range(range, &default),
                None => block.body.push_str(&default),
            }
        }
    }

    /// Read a `self:<name>(<int>)` setter from any block.
    pub fn setting(&self, name: &str) -> Option<i64> {
        let needle = format!("self:{name}(");
        for block in &self.blocks {
            if let Some(start) = block.body.find(&needle) {
                let open = start + needle.len();
                let close = block.body[open..].find(')')? + open;
                let arg = block.body[open..close].trim();
                if let Ok(v) = arg.parse::<i64>() {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Set a `self:<name>(<int>)` setter inside `block_id`, adding it if absent.
    pub fn set_setting(&mut self, block_id: &str, name: &str, value: i64) {
        let needle = format!("self:{name}(");
        let call = format!("self:{name}({value})");

        for block in &mut self.blocks {
            if let Some(start) = block.body.find(&needle) {
                let open = start + needle.len();
                if let Some(rel) = block.body[open..].find(')') {
                    let close = open + rel + 1;
                    block.body.replace_range(start..close, &call);
                    return;
                }
            }
        }

        match self.block_mut(block_id) {
            Some(block) => block.body.push_str(&call),
            None => self.set_block(block_id, call),
        }
    }
}

/// Format an alpha value the way the firmware's Lua expects (`1` or `0.5`).
fn format_alpha(alpha: f32) -> String {
    let a = alpha.clamp(0.0, 1.0);
    if (a - 1.0).abs() < f32::EPSILON {
        "1".to_string()
    } else if a == 0.0 {
        "0".to_string()
    } else {
        let s = format!("{a:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Which form of LED colour call a stored script uses.
///
/// Firmware 1.5.x stores `self:glc(layer, {{r,g,b,alpha}})` inside an
/// `--[[@sglc]]` block. Firmware 1.2.x instead stores the global five
/// argument form, `glc(num, layer, r, g, b)`, usually twice (once per layer)
/// in the element's init script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedDialect {
    /// `self:glc(layer, {{r,g,b,alpha}})`.
    Curve,
    /// `glc(num, layer, r, g, b)`.
    Global,
    /// The script does not set an LED colour.
    None,
}

/// Argument-list ranges of every global `glc(..)` call in `body`.
///
/// Only the plain global form is matched; `self:glc(..)` is the curve form
/// and is handled separately.
fn global_glc_arg_ranges(body: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;

    while let Some(rel) = body[from..].find("glc(") {
        let start = from + rel;
        from = start + 4;
        // Skip the method form and any longer identifier ending in "glc".
        if start > 0 {
            let prev = bytes[start - 1];
            if prev == b':' || prev == b'_' || prev.is_ascii_alphanumeric() {
                continue;
            }
        }
        let open = start + 3; // index of '('
        let mut depth = 0i32;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'(' | b'{' => depth += 1,
                b')' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        out.push(open + 1..i);
                        from = i + 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    out
}

/// Byte range of a `name(...)` call in `body`, including its arguments.
fn call_range(body: &str, name: &str) -> Option<std::ops::Range<usize>> {
    let start = body.find(name)?;
    let bytes = body.as_bytes();
    let mut i = start + name.len();
    while i < bytes.len() && bytes[i] != b'(' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'{' => depth += 1,
            b')' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start..i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse the `{{r,g,b,a},..}` curve out of a `glc` call.
fn parse_glc_curve(body: &str) -> Option<Vec<[i64; 4]>> {
    let start = body.find("self:glc(").map(|i| i + "self:glc(".len())?;
    let rest = &body[start..];
    let open = rest.find('{')?;
    let mut depth = 0i32;
    let mut end = None;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let table = &rest[open + 1..end? - 1];

    let mut curve = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = table[cursor..].find('{') {
        let s = cursor + rel + 1;
        let e = s + table[s..].find('}')?;
        let mut entry = [0i64; 4];
        let mut n = 0;
        for part in table[s..e].split(',') {
            if n == 4 {
                break;
            }
            entry[n] = part.trim().parse::<f64>().ok()? as i64;
            n += 1;
        }
        if n >= 3 {
            curve.push(entry);
        }
        cursor = e + 1;
    }
    (!curve.is_empty()).then_some(curve)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The firmware's default encoder event script, verbatim from
    /// GRID_ACTIONSTRING_ENCODER_ENCODER in grid_ui_encoder.h.
    const ENCODER_DEFAULT: &str = "--[[@sec]]self:emo(0)self:ev0(50)self:emi(0)self:ema(127)self:ese(100)--[[@sglc]]self:glc(-1,{{-1,-1,-1,1}})self:glp(-1,-1)--[[@gms]]self:gms(-1,-1,-1,-1)";

    #[test]
    fn parses_firmware_default_into_three_blocks() {
        let a = ActionString::parse(ENCODER_DEFAULT);
        assert_eq!(a.preamble, "");
        let ids: Vec<&str> = a.blocks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["sec", "sglc", "gms"]);
        assert_eq!(
            a.block("sglc").unwrap().body,
            "self:glc(-1,{{-1,-1,-1,1}})self:glp(-1,-1)"
        );
    }

    #[test]
    fn round_trips_byte_for_byte() {
        assert_eq!(
            ActionString::parse(ENCODER_DEFAULT).render(),
            ENCODER_DEFAULT
        );
        // And a few shapes that are not block-structured at all.
        for s in ["", "print('hi')", "--[[@cb]]--[[Encoder Init]]"] {
            assert_eq!(ActionString::parse(s).render(), s, "round-tripping {s:?}");
        }
    }

    #[test]
    fn setting_a_colour_touches_only_the_colour_call() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        a.set_led_color(Rgb::new(255, 0, 0), 1.0);
        assert_eq!(
            a.render(),
            "--[[@sec]]self:emo(0)self:ev0(50)self:emi(0)self:ema(127)self:ese(100)--[[@sglc]]self:glc(-1,{{255,0,0,1}})self:glp(-1,-1)--[[@gms]]self:gms(-1,-1,-1,-1)"
        );
        assert_eq!(a.led_color(), Some(Rgb::new(255, 0, 0)));
    }

    #[test]
    fn colour_can_be_changed_repeatedly_without_growing() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        a.set_led_color(Rgb::new(255, 0, 0), 1.0);
        let once = a.render();
        a.set_led_color(Rgb::new(255, 0, 0), 1.0);
        assert_eq!(
            a.render(),
            once,
            "re-applying the same colour is idempotent"
        );
        a.set_led_color(Rgb::new(0, 0, 255), 1.0);
        assert_eq!(a.led_color(), Some(Rgb::new(0, 0, 255)));
        assert_eq!(
            a.render().len(),
            once.len(),
            "blue is the same width as red here"
        );
    }

    #[test]
    fn default_colour_reads_as_none() {
        let a = ActionString::parse(ENCODER_DEFAULT);
        assert_eq!(a.led_color(), None, "-1 means 'element default'");
    }

    #[test]
    fn clearing_restores_the_firmware_default() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        a.set_led_color(Rgb::new(1, 2, 3), 1.0);
        a.clear_led_color();
        assert_eq!(a.render(), ENCODER_DEFAULT);
    }

    #[test]
    fn alpha_is_formatted_as_lua_numbers() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        a.set_led_color(Rgb::new(10, 20, 30), 0.5);
        assert!(
            a.render().contains("self:glc(-1,{{10,20,30,0.5}})"),
            "{}",
            a.render()
        );
        a.set_led_color(Rgb::new(10, 20, 30), 1.0);
        assert!(a.render().contains("self:glc(-1,{{10,20,30,1}})"));
    }

    #[test]
    fn adds_a_colour_block_when_absent() {
        let mut a = ActionString::parse("--[[@cb]]--[[Encoder Init]]");
        a.set_led_color(Rgb::new(0, 255, 0), 1.0);
        assert_eq!(
            a.render(),
            "--[[@cb]]--[[Encoder Init]]--[[@sglc]]self:glc(-1,{{0,255,0,1}})self:glp(-1,-1)"
        );
    }

    #[test]
    fn reads_and_writes_encoder_settings() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        assert_eq!(a.setting("emi"), Some(0));
        assert_eq!(a.setting("ema"), Some(127));
        assert_eq!(a.setting("ese"), Some(100));
        assert_eq!(a.setting("emo"), Some(0));

        a.set_setting(ENCODER_BLOCK, "ema", 100);
        a.set_setting(ENCODER_BLOCK, "emi", 10);
        assert_eq!(a.setting("ema"), Some(100));
        assert_eq!(a.setting("emi"), Some(10));
        assert!(a.render().starts_with(
            "--[[@sec]]self:emo(0)self:ev0(50)self:emi(10)self:ema(100)self:ese(100)"
        ));
    }

    #[test]
    fn adds_a_missing_setting_to_the_right_block() {
        let mut a = ActionString::parse("--[[@sec]]self:emo(0)");
        a.set_setting(ENCODER_BLOCK, "ema", 64);
        assert_eq!(a.render(), "--[[@sec]]self:emo(0)self:ema(64)");
    }

    #[test]
    fn rejects_strings_over_the_firmware_limit() {
        let mut a = ActionString::default();
        a.set_block("big", "x".repeat(ACTIONSTRING_MAXLENGTH));
        assert!(matches!(
            a.render_checked(),
            Err(Error::ActionTooLong { .. })
        ));
    }

    #[test]
    fn parses_multi_point_colour_curves() {
        let a =
            ActionString::parse("--[[@sglc]]self:glc(-1,{{0,0,0,1},{10,10,10,1},{255,128,0,1}})");
        assert_eq!(
            a.led_color(),
            Some(Rgb::new(255, 128, 0)),
            "the last point is the max colour"
        );
    }

    /// Exactly what an EN16 RevD on firmware 1.5.1 returns for element 0's
    #[test]
    fn rewrap_converts_between_the_two_storage_conventions() {
        let wrapped = "<?lua --[[@cb]] --[[Encoder Init]] ?>";
        let bare = "--[[@cb]] --[[Encoder Init]]";

        // Down-convert for 1.5.2+, up-convert for 1.5.1 and older.
        assert_eq!(rewrap(wrapped, false), bare);
        assert_eq!(rewrap(bare, true), wrapped);

        // Idempotent in both directions: already-correct text is untouched.
        assert_eq!(rewrap(bare, false), bare);
        assert_eq!(rewrap(wrapped, true), wrapped);
        assert_eq!(rewrap(&rewrap(wrapped, false), true), wrapped);
    }

    #[test]
    fn rewrap_leaves_a_script_without_the_wrapper_intact() {
        // A hand-written script must not acquire or lose anything but the
        // wrapper itself.
        let hand = "print('hello') -- no markers here";
        assert_eq!(rewrap(hand, false), hand);
        assert_eq!(rewrap(hand, true), format!("<?lua {hand} ?>"));
    }

    /// encoder event, including the `<?lua ?>` wrapper and its whitespace.
    const FROM_HARDWARE: &str = "<?lua --[[@sec]] self:emo(0) self:ev0(50) self:emi(0) self:ema(127) self:ese(100)--[[@sglc]] self:glc(-1,{{-1,-1,-1,1}}) self:glp(-1,-1)--[[@gms]] self:gms(-1,-1,-1,-1) ?>";

    #[test]
    fn round_trips_the_real_device_format() {
        let a = ActionString::parse(FROM_HARDWARE);
        assert_eq!(a.preamble, "<?lua ");
        assert_eq!(a.render(), FROM_HARDWARE);
        assert_eq!(a.setting("ema"), Some(127));
        assert_eq!(a.setting("ese"), Some(100));
        assert_eq!(a.led_color(), None);
    }

    #[test]
    fn colouring_the_real_device_format_changes_only_the_glc_call() {
        let mut a = ActionString::parse(FROM_HARDWARE);
        a.set_led_color(Rgb::new(255, 0, 0), 1.0);
        assert_eq!(
            a.render(),
            "<?lua --[[@sec]] self:emo(0) self:ev0(50) self:emi(0) self:ema(127) self:ese(100)--[[@sglc]] self:glc(-1,{{255,0,0,1}}) self:glp(-1,-1)--[[@gms]] self:gms(-1,-1,-1,-1) ?>",
            "the wrapper, spacing and every other call must survive untouched"
        );
        assert_eq!(a.led_color(), Some(Rgb::new(255, 0, 0)));

        // And it is reversible.
        a.clear_led_color();
        assert_eq!(a.render(), FROM_HARDWARE);
    }

    #[test]
    fn editing_a_setting_in_the_real_device_format_keeps_the_wrapper() {
        let mut a = ActionString::parse(FROM_HARDWARE);
        a.set_setting(ENCODER_BLOCK, "ema", 100);
        let out = a.render();
        assert!(out.starts_with("<?lua "), "{out}");
        assert!(out.ends_with(" ?>"), "{out}");
        assert!(out.contains("self:ema(100)"), "{out}");
        assert!(out.contains("self:glc(-1,{{-1,-1,-1,1}})"), "{out}");
    }

    /// The init script an EN16 RevD actually returns for an encoder.
    const INIT_FROM_HARDWARE: &str = "<?lua --[[@cb]] --[[Encoder Init]] ?>";

    #[test]
    fn appended_blocks_land_inside_the_lua_region() {
        // Appending after the `?>` terminator puts the code outside the Lua
        // region, where the firmware silently ignores it.
        let mut a = ActionString::parse(INIT_FROM_HARDWARE);
        assert_eq!(a.epilogue, " ?>");
        assert_eq!(a.render(), INIT_FROM_HARDWARE, "round-trip stays exact");

        a.set_block("gmrr", "self:gmrr('e',0,176,20,{true,true})");
        assert_eq!(
            a.render(),
            "<?lua --[[@cb]] --[[Encoder Init]]--[[@gmrr]]self:gmrr('e',0,176,20,{true,true}) ?>"
        );
        assert!(
            a.render().ends_with(" ?>"),
            "the terminator must stay last: {}",
            a.render()
        );

        assert!(a.remove_block("gmrr"));
        assert_eq!(
            a.render(),
            INIT_FROM_HARDWARE,
            "removal restores it exactly"
        );
    }

    #[test]
    fn blocks_can_be_added_and_removed_without_touching_the_rest() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        a.set_block("gmrr", "self:gmrr('e',-1,-1,-1,{true,true})");
        assert!(a.render().starts_with(ENCODER_DEFAULT));
        assert!(a
            .render()
            .ends_with("--[[@gmrr]]self:gmrr('e',-1,-1,-1,{true,true})"));

        assert!(a.remove_block("gmrr"));
        assert_eq!(a.render(), ENCODER_DEFAULT, "removal is exact");
        assert!(!a.remove_block("gmrr"), "removing twice is a no-op");
    }

    #[test]
    fn turning_the_led_off_is_black_on_every_channel() {
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        a.set_led_color(Rgb::BLACK, 1.0);
        assert!(
            a.render().contains("self:glc(-1,{{0,0,0,1}})"),
            "{}",
            a.render()
        );
        // A single-point curve derives min and mid from max, so an all-zero
        // max leaves nothing lit at any value.
        assert_eq!(a.led_color(), Some(Rgb::BLACK));
    }

    /// Exactly what an EF44 RevD on firmware 1.2.35 stores for an encoder's
    /// init event. This older firmware uses the global five-argument `glc`,
    /// twice (one call per LED layer), with the colour taken from locals.
    const EF44_INIT_FROM_HARDWARE: &str = "<?lua --[[@l]] local num,bval,eval,red,gre,blu=self:ind(),self:bva(),self:eva(),glr(),glg(),glb()--[[@glc]] glc(num,1,red,gre,blu)--[[@glc]] glc(num,2,red,gre,blu)--[[@glp]] glp(num,1,bval)--[[@glp]] glp(num,2,eval) ?>";

    #[test]
    fn recognises_both_led_dialects() {
        assert_eq!(
            ActionString::parse(ENCODER_DEFAULT).led_dialect(),
            LedDialect::Curve
        );
        assert_eq!(
            ActionString::parse(EF44_INIT_FROM_HARDWARE).led_dialect(),
            LedDialect::Global
        );
        assert_eq!(
            ActionString::parse("<?lua --[[@cb]] print('tick') ?>").led_dialect(),
            LedDialect::None
        );
    }

    #[test]
    fn global_dialect_round_trips_and_blacks_out_every_layer() {
        let mut a = ActionString::parse(EF44_INIT_FROM_HARDWARE);
        assert_eq!(a.render(), EF44_INIT_FROM_HARDWARE, "round-trip stays exact");

        assert_eq!(a.set_global_led_color(Rgb::BLACK), 2, "one call per layer");
        let out = a.render();
        assert!(out.contains("glc(num,1,0,0,0)"), "{out}");
        assert!(out.contains("glc(num,2,0,0,0)"), "{out}");
        // Everything else must survive, including the brightness calls and
        // the terminator.
        assert!(out.contains("glp(num,1,bval)"), "{out}");
        assert!(out.contains("glp(num,2,eval)"), "{out}");
        assert!(out.starts_with("<?lua "), "{out}");
        assert!(out.ends_with(" ?>"), "{out}");
        assert_eq!(a.global_led_color(), Some(Rgb::BLACK));
    }

    #[test]
    fn global_dialect_keeps_element_and_layer_arguments() {
        let mut a = ActionString::parse("--[[@glc]]glc(num,2,red,gre,blu)");
        a.set_global_led_color(Rgb::new(10, 20, 30));
        assert_eq!(a.render(), "--[[@glc]]glc(num,2,10,20,30)");
    }

    #[test]
    fn method_form_is_not_mistaken_for_the_global_one() {
        // `self:glc(..)` takes a curve, not five scalars; rewriting it as
        // though it were global would corrupt the script.
        let mut a = ActionString::parse(ENCODER_DEFAULT);
        assert_eq!(a.set_global_led_color(Rgb::BLACK), 0);
        assert_eq!(a.render(), ENCODER_DEFAULT, "left untouched");
    }

    #[test]
    fn tolerates_unknown_blocks_and_preamble() {
        let s = "prefix--[[@weird]]whatever()--[[@sglc]]self:glc(-1,{{1,2,3,1}})";
        let a = ActionString::parse(s);
        assert_eq!(a.preamble, "prefix");
        assert_eq!(a.render(), s);
        assert_eq!(a.led_color(), Some(Rgb::new(1, 2, 3)));
    }
}
