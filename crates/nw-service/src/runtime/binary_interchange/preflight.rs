//! Reject embedded essence before calling the linked-media interchange readers.
use super::Format;
use craft_timeline::time::TICKS_PER_SECOND;
use nw_domain::Result;
pub(super) fn linked_aaf(bytes: &[u8]) -> Result<()> {
    let format = Format::Aaf;
    let invalid = || format.invalid("Invalid AAF compound storage or property table");
    let container = craft_cfb::CompoundFile::open(bytes).map_err(|_| invalid())?;
    // The decoder reads the EssenceData stream from property 0x2702. Inspect every
    // object property table, including objects whose class identifier is damaged.
    let mut property_bytes = 0;
    for storage in std::iter::once(0).chain(container.walk()) {
        let Some(stream) = container.child(storage, "properties") else {
            continue;
        };
        let table = container
            .read_capped(stream, bytes.len() as u64)
            .map_err(|_| invalid())?;
        property_bytes += table.len();
        if property_bytes > bytes.len() * 4 {
            return Err(invalid());
        }
        let [0x4c, _, low, high, ..] = table.as_slice() else {
            return Err(invalid());
        };
        let count = usize::from(u16::from_le_bytes([*low, *high]));
        let values_start = 4 + 6 * count;
        let entries = table.get(4..values_start).ok_or_else(invalid)?;
        let mut values = &table[values_start..];
        for entry in entries.as_chunks::<6>().0 {
            let size = usize::from(u16::from_le_bytes([entry[4], entry[5]]));
            let (value, remainder) = values.split_at_checked(size).ok_or_else(invalid)?;
            values = remainder;
            match &entry[..4] {
                [0x02, 0x27, 0x42, 0x00] => {
                    return Err(format.unsupported(
                        "Embedded AAF essence requires a separate media extraction workflow",
                    ));
                }
                [0x01, 0x4b, 0x82, 0x00] if !exact_rate(value, i32::from_le_bytes) => {
                    return Err(format.unsupported("AAF edit rate cannot be represented exactly"));
                }
                [0x02, 0x4b, 0x82, 0x00] if value != [0; 8] => {
                    return Err(format.unsupported("AAF timeline slot origins must be zero"));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub(super) fn exact_rate(bytes: &[u8], decode: fn([u8; 4]) -> i32) -> bool {
    let Some((numerator, [a, b, c, d])) = bytes.split_first_chunk::<4>() else {
        return false;
    };
    let num = i64::from(decode(*numerator));
    let denominator = i64::from(decode([*a, *b, *c, *d]));
    num > 0
        && denominator > 0
        && i128::from(TICKS_PER_SECOND) * i128::from(denominator) % i128::from(num) == 0
}
