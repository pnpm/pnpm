use crate::Implementation;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

const ELF_HEADER_SIZE: usize = 64;
const MIN_PROGRAM_HEADER_SIZE: usize = 40;
const MAX_PROGRAM_HEADERS_SIZE: usize = 1024 * 1024;
const MAX_INTERPRETER_SIZE: usize = 4096;

/// Detect libc implementation from the ELF interpreter
/// (`/proc/self/exe` `PT_INTERP`).
pub fn detect() -> Option<Implementation> {
    let interpreter = read_elf_interpreter(&mut File::open("/proc/self/exe").ok()?)?;
    classify_interpreter(&interpreter)
}

fn read_elf_interpreter(file: &mut (impl Read + Seek)) -> Option<String> {
    let mut header = [0_u8; ELF_HEADER_SIZE];
    file.read_exact(&mut header).ok()?;
    let layout = elf_layout(&header)?;
    let table_size = layout.phentsize.checked_mul(layout.phnum)?;
    if table_size > MAX_PROGRAM_HEADERS_SIZE {
        return None;
    }
    file.seek(SeekFrom::Start(layout.phoff)).ok()?;
    let mut program_headers = vec![0_u8; table_size];
    file.read_exact(&mut program_headers).ok()?;
    let (offset, size) = interpreter_location(&program_headers, layout.phentsize)?;
    if size > MAX_INTERPRETER_SIZE {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut interpreter = vec![0_u8; size];
    file.read_exact(&mut interpreter).ok()?;
    decode_interpreter(&interpreter).map(str::to_string)
}

fn classify_interpreter(interpreter: &str) -> Option<Implementation> {
    if interpreter.contains("/ld-musl-") {
        return Some(Implementation::Musl);
    }
    if interpreter.contains("/ld-linux-") {
        return Some(Implementation::Glibc);
    }
    None
}

struct ElfLayout {
    phoff: u64,
    phentsize: usize,
    phnum: usize,
}

fn elf_layout(header: &[u8]) -> Option<ElfLayout> {
    if header.len() < ELF_HEADER_SIZE
        || header[0..4] != [0x7f, b'E', b'L', b'F']
        || header[4] != 2
        || header[5] != 1
    {
        return None;
    }
    let phoff = u64::from_le_bytes(header[32..40].try_into().ok()?);
    let phentsize = usize::from(u16::from_le_bytes(header[54..56].try_into().ok()?));
    let phnum = usize::from(u16::from_le_bytes(header[56..58].try_into().ok()?));
    if phnum == 0 || phentsize < MIN_PROGRAM_HEADER_SIZE {
        return None;
    }
    Some(ElfLayout { phoff, phentsize, phnum })
}

fn interpreter_location(program_headers: &[u8], phentsize: usize) -> Option<(u64, usize)> {
    for program_header in program_headers.chunks_exact(phentsize) {
        let p_type = u32::from_le_bytes(program_header[0..4].try_into().ok()?);
        if p_type == 3 {
            let offset = u64::from_le_bytes(program_header[8..16].try_into().ok()?);
            let size =
                u64::from_le_bytes(program_header[32..40].try_into().ok()?).try_into().ok()?;
            return Some((offset, size));
        }
    }
    None
}

fn decode_interpreter(bytes: &[u8]) -> Option<&str> {
    let interpreter = core::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
    (!interpreter.is_empty()).then_some(interpreter)
}

#[cfg(test)]
mod tests;
