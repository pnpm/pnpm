use crate::Implementation;

/// Detect libc implementation from the ELF interpreter
/// (`/proc/self/exe` `PT_INTERP`).
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
        let phdr_start = phoff.checked_add(i.checked_mul(phentsize)?)?;
        if phdr_start.checked_add(40).is_none_or(|end| end > elf.len()) {
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
            let end = start.checked_add(p_filesz)?;
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
mod tests;
