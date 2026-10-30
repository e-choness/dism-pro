//! Identifiers (TD-0001 §5.1). Both serialize as strings.

use std::{
    fmt::{self, Write as _},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{Error, units::UnixMillis};

/// Stable identity of one Windows installation (from its machine GUID or offline hive).
/// Text form: lower-case hyphenated GUID, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(pub [u8; 16]);

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_char('-')?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for TargetId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let invalid = || Error::Parse {
            what: "target id",
            value: s.to_owned(),
        };
        let groups: Vec<&str> = s.split('-').collect();
        let lens: Vec<usize> = groups.iter().map(|g| g.len()).collect();
        if lens != [8, 4, 4, 4, 12] {
            return Err(invalid());
        }
        let hex: String = groups.concat();
        // from_str_radix would also accept a leading '+'.
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let mut out = [0_u8; 16];
        for (i, byte) in out.iter_mut().enumerate() {
            let pair = hex.get(i * 2..i * 2 + 2).ok_or_else(invalid)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
        }
        Ok(Self(out))
    }
}

/// Job identifier with the ULID layout: 48-bit millisecond timestamp, then 80 random bits.
/// Ordering by `JobId` is ordering by creation time. Text form: 26 Crockford base32 chars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(u128);

const RANDOM_BITS: u32 = 80;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl JobId {
    /// Builds an id from a clock reading and caller-supplied randomness, so this crate
    /// needs neither an OS clock nor an RNG. Bits above 48 in `time` and above 80 in
    /// `random` are discarded.
    #[must_use]
    pub fn from_parts(time: UnixMillis, random: u128) -> Self {
        let time = u128::from(time.0 & ((1 << 48) - 1));
        let random = random & ((1 << RANDOM_BITS) - 1);
        Self((time << RANDOM_BITS) | random)
    }

    #[must_use]
    pub fn time(self) -> UnixMillis {
        // The shifted value has at most 48 bits, so it always fits.
        UnixMillis(u64::try_from(self.0 >> RANDOM_BITS).unwrap_or(u64::MAX))
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 26 chars × 5 bits = 130 bits; the first char carries the top 3 bits.
        for i in (0..26).rev() {
            let index = usize::try_from((self.0 >> (i * 5)) & 0x1f).unwrap_or(0);
            f.write_char(char::from(CROCKFORD[index]))?;
        }
        Ok(())
    }
}

impl FromStr for JobId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let invalid = || Error::Parse {
            what: "job id",
            value: s.to_owned(),
        };
        if s.len() != 26 {
            return Err(invalid());
        }
        let mut value: u128 = 0;
        for (i, c) in s.bytes().enumerate() {
            let digit = crockford_digit(c).ok_or_else(invalid)?;
            // The first char may only carry 3 bits (max "7").
            if i == 0 && digit > 7 {
                return Err(invalid());
            }
            value = (value << 5) | u128::from(digit);
        }
        Ok(Self(value))
    }
}

/// Crockford decoding: case-insensitive, `I`/`L` → 1, `O` → 0, `U` rejected.
fn crockford_digit(c: u8) -> Option<u8> {
    let c = c.to_ascii_uppercase();
    let c = match c {
        b'I' | b'L' => b'1',
        b'O' => b'0',
        _ => c,
    };
    CROCKFORD
        .iter()
        .position(|&d| d == c)
        .and_then(|p| u8::try_from(p).ok())
}

macro_rules! string_serde {
    ($t:ty) => {
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                s.parse().map_err(D::Error::custom)
            }
        }
    };
}

string_serde!(TargetId);
string_serde!(JobId);

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn target_id_text_round_trip() {
        let id = TargetId([
            0x53, 0xbf, 0xae, 0x52, 0xb1, 0x67, 0x4e, 0x2f, 0xa2, 0x58, 0x0a, 0x37, 0xb5, 0x7f,
            0xf8, 0x45,
        ]);
        let text = id.to_string();
        assert_eq!(text, "53bfae52-b167-4e2f-a258-0a37b57ff845");
        assert_eq!(text.to_uppercase().parse::<TargetId>().unwrap(), id);
        assert!(
            "53bfae52b1674e2fa2580a37b57ff845"
                .parse::<TargetId>()
                .is_err()
        );
        assert!(
            "zzbfae52-b167-4e2f-a258-0a37b57ff845"
                .parse::<TargetId>()
                .is_err()
        );
        assert!(
            "+3bfae52-b167-4e2f-a258-0a37b57ff845"
                .parse::<TargetId>()
                .is_err()
        );
    }

    #[test]
    fn job_id_known_vector() {
        assert_eq!(
            JobId::from_parts(UnixMillis(0), 0).to_string(),
            "00000000000000000000000000"
        );
        let max = JobId::from_parts(UnixMillis(u64::MAX), u128::MAX);
        assert_eq!(max.to_string(), "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
        assert!("8ZZZZZZZZZZZZZZZZZZZZZZZZZ".parse::<JobId>().is_err());
        assert!("0000000000000000000000000U".parse::<JobId>().is_err());
    }

    proptest! {
        #[test]
        fn job_id_round_trips_and_sorts_by_time(t1 in 0_u64..1 << 48, t2 in 0_u64..1 << 48, r1: u128, r2: u128) {
            let a = JobId::from_parts(UnixMillis(t1), r1);
            let b = JobId::from_parts(UnixMillis(t2), r2);
            prop_assert_eq!(a.to_string().parse::<JobId>().unwrap(), a);
            prop_assert_eq!(a.to_string().to_lowercase().parse::<JobId>().unwrap(), a);
            prop_assert_eq!(a.time(), UnixMillis(t1));
            if t1 < t2 {
                prop_assert!(a < b);
            }
        }
    }

    #[test]
    fn ids_serialize_as_strings() {
        let id = JobId::from_parts(UnixMillis(1), 2);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<JobId>(&json).unwrap(), id);
    }
}
