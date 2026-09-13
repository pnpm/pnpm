pub(super) fn write_map_header(writer: &mut Vec<u8>, n: usize) {
    if n < 16 {
        writer.push(0x80 | (n as u8));
    } else if u16::try_from(n).is_ok() {
        writer.push(0xde);
        writer.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        // MessagePack's `map 32` header caps length at `u32::MAX`. On
        // 64-bit hosts a `usize` could in principle exceed that; use
        // a checked conversion so we panic with a clear message
        // rather than silently truncating to a corrupt payload.
        let n = u32::try_from(n).expect("map length exceeds MessagePack's u32::MAX limit");
        writer.push(0xdf);
        writer.extend_from_slice(&n.to_be_bytes());
    }
}

pub(super) fn write_str(writer: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len();
    if n < 32 {
        writer.push(0xa0 | (n as u8));
    } else if u8::try_from(n).is_ok() {
        writer.push(0xd9);
        writer.push(n as u8);
    } else if u16::try_from(n).is_ok() {
        writer.push(0xda);
        writer.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        // `str 32` tops out at `u32::MAX` bytes. Checked cast to
        // fail loudly rather than silently truncating to a corrupt
        // length prefix.
        let n = u32::try_from(n).expect("string length exceeds MessagePack's u32::MAX limit");
        writer.push(0xdb);
        writer.extend_from_slice(&n.to_be_bytes());
    }
    writer.extend_from_slice(bytes);
}
