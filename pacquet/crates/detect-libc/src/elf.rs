use crate::Implementation;

/// Detect libc implementation from the ELF interpreter
/// (`/proc/self/exe` PT_INTERP).
///
/// Returns `Some(Implementation::Musl)` when the interpreter path
/// contains `"/ld-musl-"`, `Some(Implementation::Glibc)` when it
/// contains `"/ld-linux-"`, `None` otherwise.
pub fn detect() -> Option<Implementation> {
    let exe_path = std::fs::read_link("/proc/self/exe").ok()?;
    let data = std::fs::read(&exe_path).ok()?;
    let interp = elf_interpreter(&data)?;
    classify_interpreter(interp)
}

fn classify_interpreter(interp: &str) -> Option<Implementation> {
    if interp.contains("/ld-musl-") {
        return Some(Implementation::Musl);
    }
    if interp.contains("/ld-linux-") {
        return Some(Implementation::Glibc);
    }
    None
}

fn elf_interpreter(elf: &[u8]) -> Option<&str> {
    if elf.len() < 64 {
        return None;
    }
    if elf[0..4] != [0x7f, b'E', b'L', b'F'] {
        return None;
    }
    if elf[4] != 2 {
        return None;
    }
    if elf[5] != 1 {
        return None;
    }

    let phoff = u64::from_le_bytes(elf[32..40].try_into().ok()?);
    let phentsize = u16::from_le_bytes(elf[54..56].try_into().ok()?);
    let phnum = u16::from_le_bytes(elf[56..58].try_into().ok()?);

    let phoff: usize = phoff.try_into().ok()?;
    let phentsize = usize::from(phentsize);
    let phnum = usize::from(phnum);

    for i in 0..phnum {
        let phdr_start = phoff + i * phentsize;
        if phdr_start + 8 > elf.len() {
            break;
        }
        let p_type = u32::from_le_bytes(elf[phdr_start..phdr_start + 4].try_into().ok()?);
        if p_type == 3 {
            let p_offset =
                u64::from_le_bytes(elf[phdr_start + 8..phdr_start + 16].try_into().ok()?);
            let p_filesz =
                u64::from_le_bytes(elf[phdr_start + 32..phdr_start + 40].try_into().ok()?);
            let start: usize = p_offset.try_into().ok()?;
            let p_filesz: usize = p_filesz.try_into().ok()?;
            let end = start + p_filesz;
            if end <= elf.len() {
                let interp = core::str::from_utf8(&elf[start..end]).ok()?;
                let trimmed = interp.trim_end_matches('\0');
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{classify_interpreter, elf_interpreter};
    use crate::Implementation;

    fn build_elf_with_interp(interp: &[u8]) -> Vec<u8> {
        let phoff: u64 = 64;
        let interp_offset = phoff + 56;

        let buf_size = usize::try_from(interp_offset).unwrap() + interp.len();
        let mut elf = vec![0u8; buf_size];

        elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 2;
        elf[5] = 1;
        elf[6] = 1;
        elf[16..18].copy_from_slice(&3u16.to_le_bytes());
        elf[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[32..40].copy_from_slice(&phoff.to_le_bytes());
        elf[52..54].copy_from_slice(&64u16.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1u16.to_le_bytes());

        let p_offset = interp_offset;
        let p_filesz: u64 = interp.len().try_into().unwrap();
        let ph_offset: usize = phoff.try_into().unwrap();
        elf[ph_offset..ph_offset + 4].copy_from_slice(&3u32.to_le_bytes());
        elf[ph_offset + 4..ph_offset + 8].copy_from_slice(&4u32.to_le_bytes());
        elf[ph_offset + 8..ph_offset + 16].copy_from_slice(&p_offset.to_le_bytes());
        elf[ph_offset + 32..ph_offset + 40].copy_from_slice(&p_filesz.to_le_bytes());
        elf[ph_offset + 40..ph_offset + 48].copy_from_slice(&p_filesz.to_le_bytes());
        elf[ph_offset + 48..ph_offset + 56].copy_from_slice(&1u64.to_le_bytes());

        let interp_start: usize = interp_offset.try_into().unwrap();
        elf[interp_start..interp_start + interp.len()].copy_from_slice(interp);

        elf
    }

    fn build_elf_without_interp() -> Vec<u8> {
        let mut elf = vec![0u8; 64];
        elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 2;
        elf[5] = 1;
        elf[6] = 1;
        elf[16..18].copy_from_slice(&3u16.to_le_bytes());
        elf[18..20].copy_from_slice(&0x3eu16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[56..58].copy_from_slice(&0u16.to_le_bytes());
        elf
    }

    fn build_32bit_elf() -> Vec<u8> {
        let mut elf = vec![0u8; 52];
        elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 1;
        elf[5] = 1;
        elf
    }

    fn build_big_endian_elf() -> Vec<u8> {
        let mut elf = vec![0u8; 64];
        elf[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        elf[4] = 2;
        elf[5] = 2;
        elf
    }

    #[test]
    fn classify_interpreter_glibc() {
        assert_eq!(
            classify_interpreter("/lib64/ld-linux-x86-64.so.2"),
            Some(Implementation::Glibc),
        );
    }

    #[test]
    fn classify_interpreter_musl() {
        assert_eq!(classify_interpreter("/lib/ld-musl-x86_64.so.1"), Some(Implementation::Musl),);
    }

    #[test]
    fn classify_interpreter_unknown() {
        assert_eq!(classify_interpreter("/lib/ld-fallback-x86_64.so.1"), None);
    }

    #[test]
    fn classify_interpreter_empty() {
        assert_eq!(classify_interpreter(""), None);
    }

    #[test]
    fn too_small() {
        assert_eq!(elf_interpreter(&[0; 63]), None);
    }

    #[test]
    fn not_elf() {
        assert_eq!(elf_interpreter(&[0; 64]), None);
    }

    #[test]
    fn valid_elf_32bit() {
        assert_eq!(elf_interpreter(&build_32bit_elf()), None);
    }

    #[test]
    fn valid_elf_big_endian() {
        assert_eq!(elf_interpreter(&build_big_endian_elf()), None);
    }

    #[test]
    fn valid_elf_without_pt_interp() {
        assert_eq!(elf_interpreter(&build_elf_without_interp()), None);
    }

    #[test]
    fn glibc_pt_interp() {
        let interp = b"/lib64/ld-linux-x86-64.so.2\0";
        let elf = build_elf_with_interp(interp);
        assert_eq!(elf_interpreter(&elf), Some("/lib64/ld-linux-x86-64.so.2"),);
    }

    #[test]
    fn musl_pt_interp() {
        let interp = b"/lib/ld-musl-x86_64.so.1\0";
        let elf = build_elf_with_interp(interp);
        assert_eq!(elf_interpreter(&elf), Some("/lib/ld-musl-x86_64.so.1"),);
    }

    #[test]
    fn non_elf64_path_not_confused_by_magic() {
        let not_elf =
            b"\x7fELFxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert_eq!(elf_interpreter(not_elf), None);
    }
}
