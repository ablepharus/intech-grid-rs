//! Parsing element selectors like `all`, `3`, `0-5` and `0,2,7`.

use intech_grid::Module;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    All,
    List(Vec<u8>),
}

impl Selector {
    /// Resolve to concrete element indices on `module`.
    ///
    /// `all` expands to every element that has an LED, which is what every
    /// command using a selector operates on.
    pub fn resolve(&self, module: &Module) -> Result<Vec<u8>, String> {
        match self {
            Selector::All => Ok(module.led_elements().collect()),
            Selector::List(items) => {
                let count = module.element_count();
                for &i in items {
                    if i >= count {
                        return Err(format!(
                            "element {i} does not exist on {} (valid range 0-{})",
                            module.name(),
                            count.saturating_sub(1)
                        ));
                    }
                }
                Ok(items.clone())
            }
        }
    }
}

impl std::str::FromStr for Selector {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("all") || t == "*" {
            return Ok(Selector::All);
        }

        let mut out = Vec::new();
        for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if let Some((a, b)) = part.split_once('-') {
                let start: u8 = a
                    .trim()
                    .parse()
                    .map_err(|_| format!("{a:?} is not an element index"))?;
                let end: u8 = b
                    .trim()
                    .parse()
                    .map_err(|_| format!("{b:?} is not an element index"))?;
                if end < start {
                    return Err(format!("range {part:?} runs backwards"));
                }
                out.extend(start..=end);
            } else {
                out.push(
                    part.parse()
                        .map_err(|_| format!("{part:?} is not an element index or 'all'"))?,
                );
            }
        }
        if out.is_empty() {
            return Err(format!("{s:?} selects no elements"));
        }
        out.dedup();
        Ok(Selector::List(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_forms() {
        assert_eq!("all".parse::<Selector>().unwrap(), Selector::All);
        assert_eq!("ALL".parse::<Selector>().unwrap(), Selector::All);
        assert_eq!("*".parse::<Selector>().unwrap(), Selector::All);
        assert_eq!("3".parse::<Selector>().unwrap(), Selector::List(vec![3]));
        assert_eq!(
            "0-3".parse::<Selector>().unwrap(),
            Selector::List(vec![0, 1, 2, 3])
        );
        assert_eq!(
            "0,2,5".parse::<Selector>().unwrap(),
            Selector::List(vec![0, 2, 5])
        );
        assert_eq!(
            "0-2,7".parse::<Selector>().unwrap(),
            Selector::List(vec![0, 1, 2, 7])
        );
    }

    #[test]
    fn rejects_nonsense() {
        for bad in ["", "x", "3-1", "1,,x", "-"] {
            assert!(bad.parse::<Selector>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn all_expands_to_led_elements_only() {
        let en16 = Module::from_hwcfg(0xc1).unwrap();
        let resolved = Selector::All.resolve(en16).unwrap();
        assert_eq!(resolved.len(), 16, "the system element has no LED");
        assert_eq!(resolved[0], 0);
        assert_eq!(resolved[15], 15);
    }

    #[test]
    fn rejects_out_of_range_elements() {
        let en16 = Module::from_hwcfg(0xc1).unwrap();
        let err = Selector::List(vec![17]).resolve(en16).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }
}
