use super::MAGIC;
use std::{
    fs,
    io::{
        self,
        Write,
    },
    os::unix::fs::PermissionsExt,
    path::Path,
};

/// Writes the archive of the file tree at `path` to `writer`.
pub fn pack(path: &Path, writer: &mut impl Write) -> io::Result<()> {
    write_string(writer, MAGIC.as_bytes())?;
    pack_node(path, writer)
}

fn pack_node(path: &Path, writer: &mut impl Write) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    write_strings(writer, &["(", "type"])?;
    if metadata.is_symlink() {
        write_strings(writer, &["symlink", "target"])?;
        write_string(writer, fs::read_link(path)?.as_os_str().as_encoded_bytes())?;
    } else if metadata.is_dir() {
        write_strings(writer, &["directory"])?;
        pack_entries(path, writer)?;
    } else {
        write_strings(writer, &["regular"])?;
        if metadata.permissions().mode() & 0o111 != 0 {
            write_strings(writer, &["executable", ""])?;
        }
        write_strings(writer, &["contents"])?;
        write_string(writer, &fs::read(path)?)?;
    }
    write_strings(writer, &[")"])
}

fn pack_entries(directory: &Path, writer: &mut impl Write) -> io::Result<()> {
    let mut names = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        write_strings(writer, &["entry", "(", "name"])?;
        write_string(writer, name.as_encoded_bytes())?;
        write_strings(writer, &["node"])?;
        pack_node(&directory.join(&name), writer)?;
        write_strings(writer, &[")"])?;
    }
    Ok(())
}

fn write_strings(writer: &mut impl Write, strings: &[&str]) -> io::Result<()> {
    strings.iter().try_for_each(|string| write_string(writer, string.as_bytes()))
}

fn write_string(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(&(bytes.len() as u64).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.write_all(&[0_u8; 8][..(8 - bytes.len() % 8) % 8])
}
