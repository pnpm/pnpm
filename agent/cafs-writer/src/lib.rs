#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::path::PathBuf;

#[cfg(not(target_os = "linux"))]
use std::{
  fs::OpenOptions,
  io::Write,
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Parse an uncompressed /v1/files payload and write each file to the CAFS.
///
/// Payload format (same as the Node-side parser in worker/src/start.ts):
///   [4-byte BE u32: JSON header length]
///   [N bytes: JSON header (ignored — reserved for future use)]
///   [entries...]
///   [64 zero bytes: end marker]
///
/// Each entry:
///   [64 bytes: SHA-512 digest, raw binary]
///   [4-byte BE u32: content length]
///   [1 byte: 0x00 = regular, 0x01 = executable]
///   [N bytes: content]
///
/// Returns the number of files newly written. Files already present (another
/// worker won the race) are silently skipped via O_EXCL and not counted.
#[napi]
pub fn write_files(store_dir: String, payload: Buffer) -> Result<u32> {
  let bytes: &[u8] = &payload;
  let entries = parse_payload(bytes)
    .map_err(|e| Error::from_reason(format!("cafs-writer parse error: {e}")))?;

  let files_dir = PathBuf::from(store_dir).join("files");
  pre_create_parent_dirs(&files_dir, &entries)
    .map_err(|e| Error::from_reason(format!("cafs-writer mkdir error: {e}")))?;

  #[cfg(target_os = "linux")]
  {
    linux_uring::write_all(&files_dir, &entries)
      .map_err(|e| Error::from_reason(format!("cafs-writer (io_uring) write error: {e}")))
  }
  #[cfg(not(target_os = "linux"))]
  {
    write_all_std(&files_dir, &entries)
      .map_err(|e| Error::from_reason(format!("cafs-writer write error: {e}")))
  }
}

pub(crate) struct Entry<'a> {
  pub digest_hex: String,
  pub executable: bool,
  pub content: &'a [u8],
}

impl Entry<'_> {
  pub fn filename(&self) -> String {
    let suffix = &self.digest_hex[2..];
    if self.executable {
      format!("{suffix}-exec")
    } else {
      suffix.to_string()
    }
  }

  pub fn prefix(&self) -> &str {
    &self.digest_hex[..2]
  }

  pub fn mode(&self) -> u32 {
    if self.executable { 0o755 } else { 0o644 }
  }
}

fn parse_payload(bytes: &[u8]) -> std::result::Result<Vec<Entry<'_>>, String> {
  if bytes.len() < 4 {
    return Err("payload too small for header length prefix".into());
  }
  let json_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
  let mut pos = 4 + json_len;
  if pos > bytes.len() {
    return Err("payload truncated inside JSON header".into());
  }

  let mut entries = Vec::new();
  loop {
    if pos + 64 > bytes.len() {
      return Err("payload truncated at digest boundary".into());
    }
    let digest = &bytes[pos..pos + 64];
    if digest.iter().all(|&b| b == 0) {
      break;
    }
    pos += 64;

    if pos + 5 > bytes.len() {
      return Err("payload truncated at size/mode header".into());
    }
    let size = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
      as usize;
    let executable = (bytes[pos + 4] & 0x01) != 0;
    pos += 5;

    if pos + size > bytes.len() {
      return Err("payload truncated inside file content".into());
    }
    let content = &bytes[pos..pos + size];
    pos += size;

    entries.push(Entry {
      digest_hex: hex_encode(digest),
      executable,
      content,
    });
  }
  Ok(entries)
}

// The store layout normally pre-creates files/XX/ subdirectories (see
// worker init-store), but the CAFS may be empty on first agent-client use.
// Create each needed prefix dir once up front — both the std and io_uring
// paths benefit from not having to handle ENOENT on every file open.
fn pre_create_parent_dirs(
  files_dir: &std::path::Path,
  entries: &[Entry<'_>],
) -> std::io::Result<()> {
  let mut seen = std::collections::HashSet::<&str>::new();
  for e in entries {
    if seen.insert(e.prefix()) {
      std::fs::create_dir_all(files_dir.join(e.prefix()))?;
    }
  }
  Ok(())
}

#[cfg(not(target_os = "linux"))]
fn write_all_std(
  files_dir: &std::path::Path,
  entries: &[Entry<'_>],
) -> std::result::Result<u32, String> {
  use rayon::prelude::*;
  entries
    .par_iter()
    .map(|entry| write_one_std(files_dir, entry))
    .try_reduce(|| 0u32, |a, b| Ok(a + b))
}

#[cfg(not(target_os = "linux"))]
fn write_one_std(
  files_dir: &std::path::Path,
  entry: &Entry,
) -> std::result::Result<u32, String> {
  let path = files_dir.join(entry.prefix()).join(entry.filename());
  let mut opts = OpenOptions::new();
  opts.write(true).create_new(true);
  #[cfg(unix)]
  opts.mode(entry.mode());
  match opts.open(&path) {
    Ok(mut f) => {
      f.write_all(entry.content)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
      Ok(1)
    }
    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(0),
    Err(e) => Err(format!("open {}: {e}", path.display())),
  }
}

#[cfg(target_os = "linux")]
mod linux_uring {
  //! io_uring write backend.
  //!
  //! Opens + writes + closes each file via tokio-uring SQEs. The colleague's
  //! benchmark data (1000 small files: rayon+std 39s vs tokio_uring 27s)
  //! suggests the overlap-syscalls pattern dominates once file count is high.
  //!
  //! Not yet tuned: we spawn one task per file. The faster variant in the
  //! colleague's gist used a `tokio_uring::builder` with SQPOLL; worth trying
  //! next if this doesn't meet targets.
  use super::Entry;
  use std::path::Path;

  pub(super) fn write_all(
    files_dir: &Path,
    entries: &[Entry<'_>],
  ) -> std::result::Result<u32, String> {
    tokio_uring::start(async {
      let mut tasks = Vec::with_capacity(entries.len());
      for entry in entries {
        let path = files_dir.join(entry.prefix()).join(entry.filename());
        let content = entry.content.to_vec();
        let executable = entry.executable;
        tasks.push(tokio_uring::spawn(async move {
          // tokio-uring 0.5's OpenOptions doesn't expose a .mode() setter,
          // so we take the umask default (0o666 & ~umask ≈ 0o644) for
          // non-exec files and explicitly chmod executables after writing.
          // For the CAFS's purposes, non-exec=0o644 / exec=0o755 is all
          // that matters.
          let file = tokio_uring::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await;
          let file = match file {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
              return Ok::<u32, String>(0)
            }
            Err(e) => return Err(format!("open {}: {e}", path.display())),
          };
          let (res, _buf) = file.write_all_at(content, 0).await;
          res.map_err(|e| format!("write {}: {e}", path.display()))?;
          file
            .close()
            .await
            .map_err(|e| format!("close {}: {e}", path.display()))?;
          if executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
              .map_err(|e| format!("chmod {}: {e}", path.display()))?;
          }
          Ok(1)
        }));
      }
      let mut total = 0u32;
      for task in tasks {
        total += task
          .await
          .map_err(|e| format!("tokio-uring task join: {e}"))??;
      }
      Ok(total)
    })
  }
}

// Small, stable hex encoder — avoids pulling in the `hex` crate.
fn hex_encode(bytes: &[u8]) -> String {
  const HEX: &[u8; 16] = b"0123456789abcdef";
  let mut out = String::with_capacity(bytes.len() * 2);
  for &b in bytes {
    out.push(HEX[(b >> 4) as usize] as char);
    out.push(HEX[(b & 0x0f) as usize] as char);
  }
  out
}
