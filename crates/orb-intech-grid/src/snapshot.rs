//! Whole-device configuration snapshots, for dumping to and restoring from JSON.

use crate::action::ActionString;
use crate::color::Rgb;
use crate::device::Device;
use crate::error::Result;
use crate::proto::EventType;

/// One stored script.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventSnapshot {
    pub event: EventType,
    /// The Lua action string exactly as the device stores it.
    pub action: String,
}

/// One element and all of its scripts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ElementSnapshot {
    pub index: u8,
    pub kind: String,
    /// Colour parsed out of the element's LED block, when it is a literal.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub led_color: Option<Rgb>,
    pub events: Vec<EventSnapshot>,
}

/// A page of elements.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PageSnapshot {
    pub page: u8,
    pub elements: Vec<ElementSnapshot>,
}

/// A full dump of a module's configuration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub module: String,
    pub hwcfg: u8,
    pub firmware: String,
    pub pages: Vec<PageSnapshot>,
}

impl Snapshot {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
}

/// Read every script of every element on `page`.
pub fn dump_page(dev: &mut Device, page: u8) -> Result<PageSnapshot> {
    let module = dev.module();
    let mut elements = Vec::new();

    for index in 0..module.element_count() {
        let kind = module.element_kind(index)?;
        let mut events = Vec::new();
        for &event in kind.events() {
            let action = dev.fetch_action(page, index, event)?;
            events.push(EventSnapshot { event, action });
        }
        let led_color = kind
            .value_event()
            .and_then(|ev| events.iter().find(|e| e.event == ev))
            .and_then(|e| ActionString::parse(&e.action).led_color());

        elements.push(ElementSnapshot {
            index,
            kind: kind.as_str().to_string(),
            led_color,
            events,
        });
    }

    Ok(PageSnapshot { page, elements })
}

/// Read the given pages, or just the active one when `pages` is empty.
pub fn dump(dev: &mut Device, pages: &[u8]) -> Result<Snapshot> {
    let info = dev.info().clone();
    let targets: Vec<u8> = if pages.is_empty() {
        vec![dev.active_page()?]
    } else {
        pages.to_vec()
    };

    let mut out = Vec::new();
    for p in targets {
        out.push(dump_page(dev, p)?);
    }

    Ok(Snapshot {
        module: info.module,
        hwcfg: info.hwcfg,
        firmware: info.firmware,
        pages: out,
    })
}

/// Result of applying a snapshot.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Scripts written because they differed from the device.
    pub written: Vec<(u8, u8, EventType)>,
    /// Scripts skipped because the device already matched.
    pub unchanged: usize,
}

/// Write a snapshot back to the device, skipping anything already identical.
///
/// Changes land in RAM; call [`Device::store_page`] to persist them.
pub fn apply(dev: &mut Device, snapshot: &Snapshot) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();

    for page in &snapshot.pages {
        for element in &page.elements {
            for event in &element.events {
                let current = dev.fetch_action(page.page, element.index, event.event)?;
                if current == event.action {
                    report.unchanged += 1;
                    continue;
                }
                dev.write_action(page.page, element.index, event.event, &event.action)?;
                report
                    .written
                    .push((page.page, element.index, event.event));
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            module: "EN16 RevD".into(),
            hwcfg: 0xc1,
            firmware: "1.5.1".into(),
            pages: vec![PageSnapshot {
                page: 0,
                elements: vec![ElementSnapshot {
                    index: 0,
                    kind: "encoder".into(),
                    led_color: Some(Rgb::new(255, 0, 0)),
                    events: vec![EventSnapshot {
                        event: EventType::Encoder,
                        action: "--[[@sglc]]self:glc(-1,{{255,0,0,1}})".into(),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn snapshot_json_round_trips() {
        let s = sample();
        let json = s.to_json().unwrap();
        assert_eq!(Snapshot::from_json(&json).unwrap(), s);
    }

    #[test]
    fn json_keeps_action_strings_verbatim() {
        let json = sample().to_json().unwrap();
        assert!(json.contains("self:glc(-1,{{255,0,0,1}})"), "{json}");
    }
}
