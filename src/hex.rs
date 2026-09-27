/// crc32 as 8 upper-case hex digits (the DAT / `check` spelling) instead of serde's
/// default decimal, so index and manifest values can be compared by eye against a DAT.
pub mod u32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{:08X}", v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        let s = String::deserialize(d)?;
        u32::from_str_radix(&s, 16).map_err(serde::de::Error::custom)
    }
}
