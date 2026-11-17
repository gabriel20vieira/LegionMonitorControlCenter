//! Minimal EDID (VESA E-EDID 1.3/1.4 base block) reader.

#[derive(Clone, Debug, Default)]
pub struct Edid {
    pub manufacturer: String,
    pub product: u16,
    pub serial_number: u32,
    pub serial_text: Option<String>,
    pub name: Option<String>,
    pub year: u16,
}

const HEADER: [u8; 8] = [0, 255, 255, 255, 255, 255, 255, 0];

pub fn parse(b: &[u8]) -> Option<Edid> {
    if b.len() < 128 || b[0..8] != HEADER {
        return None;
    }
    let m = u16::from_be_bytes([b[8], b[9]]);
    let letter = |v: u16| (((v & 0x1f) as u8).wrapping_add(b'A' - 1)) as char;
    let manufacturer = format!("{}{}{}", letter(m >> 10), letter(m >> 5), letter(m));
    let product = u16::from_le_bytes([b[10], b[11]]);
    let serial_number = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
    let year = b[17] as u16 + 1990;
    let mut out = Edid {
        manufacturer,
        product,
        serial_number,
        serial_text: None,
        name: None,
        year,
    };
    for off in [54usize, 72, 90, 108] {
        if b[off] == 0 && b[off + 1] == 0 {
            let tag = b[off + 3];
            let raw = &b[off + 5..off + 18];
            let end = raw.iter().position(|&c| c == b'\n').unwrap_or(raw.len());
            let text = String::from_utf8_lossy(&raw[..end]).trim().to_string();
            if text.is_empty() {
                continue;
            }
            match tag {
                0xFF => out.serial_text = Some(text),
                0xFC => out.name = Some(text),
                _ => {}
            }
        }
    }
    Some(out)
}
