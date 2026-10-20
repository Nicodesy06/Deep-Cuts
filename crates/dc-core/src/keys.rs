//! Musical key notation: parsing of common notations and Camelot conversion.

/// A key as pitch class of the tonic (0 = C) and mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub tonic: u8,
    pub minor: bool,
}

const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

// Camelot number for each tonic pitch class.
const CAMELOT_MAJOR: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
const CAMELOT_MINOR: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];

impl Key {
    pub fn camelot(&self) -> String {
        let n = if self.minor {
            CAMELOT_MINOR
        } else {
            CAMELOT_MAJOR
        }[self.tonic as usize];
        format!("{}{}", n, if self.minor { 'A' } else { 'B' })
    }

    /// Open Key notation: Camelot 8A/8B correspond to 1m/1d.
    pub fn open_key(&self) -> String {
        let n = if self.minor {
            CAMELOT_MINOR
        } else {
            CAMELOT_MAJOR
        }[self.tonic as usize];
        let open = ((n as i32 - 8).rem_euclid(12) + 1) as u8;
        format!("{}{}", open, if self.minor { 'm' } else { 'd' })
    }

    /// Standard name, e.g. "Am", "F#", "Bbm".
    pub fn name(&self) -> String {
        // Flats read better for these tonics in DJ software conventions.
        let names = if matches!(self.tonic, 1 | 3 | 8 | 10) && !self.minor {
            FLAT_NAMES
        } else {
            SHARP_NAMES
        };
        let names = if self.minor && matches!(self.tonic, 3 | 10) {
            FLAT_NAMES
        } else {
            names
        };
        format!(
            "{}{}",
            names[self.tonic as usize],
            if self.minor { "m" } else { "" }
        )
    }

    pub fn from_camelot(n: u8, minor: bool) -> Option<Key> {
        if !(1..=12).contains(&n) {
            return None;
        }
        let table = if minor { CAMELOT_MINOR } else { CAMELOT_MAJOR };
        table.iter().position(|&c| c == n).map(|t| Key {
            tonic: t as u8,
            minor,
        })
    }

    /// Parse Camelot ("8A"), Open Key ("1m"), or note notation
    /// ("Am", "A minor", "F#maj", "Bbm", "Ebmin").
    pub fn parse(input: &str) -> Option<Key> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        let lower = s.to_ascii_lowercase();
        // Camelot / Open Key: digits followed by a letter.
        let digits: String = lower.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let n: u8 = digits.parse().ok()?;
            let rest = lower[digits.len()..].trim();
            return match rest {
                "a" => Key::from_camelot(n, true),
                "b" => Key::from_camelot(n, false),
                "m" | "d" => {
                    if !(1..=12).contains(&n) {
                        return None;
                    }
                    let cam = ((n as i32 - 1 + 7).rem_euclid(12) + 1) as u8;
                    Key::from_camelot(cam, rest == "m")
                }
                _ => None,
            };
        }
        let mut chars = s.chars();
        let letter = chars.next()?.to_ascii_uppercase();
        let base: i32 = match letter {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let mut rest: String = chars.collect();
        let mut tonic = base;
        if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
            tonic += 1;
            rest = r.to_string();
        } else if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
            tonic -= 1;
            rest = r.to_string();
        }
        let mode = rest.trim().to_ascii_lowercase();
        let minor = match mode.as_str() {
            "" | "maj" | "major" | "dur" => false,
            "m" | "min" | "minor" | "moll" | "-" => true,
            _ => return None,
        };
        Some(Key {
            tonic: tonic.rem_euclid(12) as u8,
            minor,
        })
    }
}

/// Camelot code for any recognised notation.
pub fn to_camelot(input: &str) -> Option<String> {
    Key::parse(input).map(|k| k.camelot())
}

/// Harmonic compatibility on the Camelot wheel: same key, ±1 on the same
/// ring, or relative major/minor.
pub fn camelot_compatible(a: &str, b: &str) -> bool {
    let (Some(ka), Some(kb)) = (Key::parse(a), Key::parse(b)) else {
        return false;
    };
    let num = |k: &Key| if k.minor { CAMELOT_MINOR } else { CAMELOT_MAJOR }[k.tonic as usize] as i32;
    let (na, nb) = (num(&ka), num(&kb));
    if ka.minor == kb.minor {
        let d = (na - nb).rem_euclid(12);
        d == 0 || d == 1 || d == 11
    } else {
        na == nb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_round_trip() {
        assert_eq!(to_camelot("Am").as_deref(), Some("8A"));
        assert_eq!(to_camelot("C").as_deref(), Some("8B"));
        assert_eq!(to_camelot("F#m").as_deref(), Some("11A"));
        assert_eq!(to_camelot("Gbm").as_deref(), Some("11A"));
        assert_eq!(to_camelot("Bb minor").as_deref(), Some("3A"));
        assert_eq!(to_camelot("8a").as_deref(), Some("8A"));
        assert_eq!(to_camelot("1m").as_deref(), Some("8A"));
        assert_eq!(to_camelot("1d").as_deref(), Some("8B"));
        assert_eq!(to_camelot("6m").as_deref(), Some("1A"));
        assert_eq!(to_camelot("E major").as_deref(), Some("12B"));
        assert_eq!(to_camelot("xyz"), None);
        assert_eq!(to_camelot("13A"), None);
        for n in 1..=12 {
            for minor in [true, false] {
                let k = Key::from_camelot(n, minor).unwrap();
                assert_eq!(Key::parse(&k.name()), Some(k));
                assert_eq!(Key::parse(&k.open_key()), Some(k), "{}", k.open_key());
            }
        }
    }

    #[test]
    fn compatibility() {
        assert!(camelot_compatible("8A", "9A"));
        assert!(camelot_compatible("12A", "1A"));
        assert!(camelot_compatible("8A", "8B"));
        assert!(!camelot_compatible("8A", "9B"));
        assert!(!camelot_compatible("8A", "10A"));
    }
}
