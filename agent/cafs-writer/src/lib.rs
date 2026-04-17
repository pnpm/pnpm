#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Serialize;
use std::io::Read;
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
  write_payload_to_cafs(&store_dir, &payload)
}

/// One digest to request from the agent's `/v1/files` endpoint.
#[napi(object)]
pub struct DigestSpec {
  pub digest: String,
  pub size: u32,
  pub executable: bool,
}

/// POST the given digest list to `{agent_url}/v1/files`, gunzip the response,
/// parse it, and write each file into the CAFS in parallel.
///
/// This collapses the HTTP request, gzip decode, parse, and write steps into
/// a single NAPI call — the JS side only splits into batches and invokes
/// this once per batch. Same wire protocol as the Node-side fetch in
/// worker/src/start.ts, same on-disk output.
#[napi]
pub fn fetch_batch(
  agent_url: String,
  digests: Vec<DigestSpec>,
  store_dir: String,
) -> Result<u32> {
  #[derive(Serialize)]
  struct RequestDigest<'a> {
    digest: &'a str,
    size: u32,
    executable: bool,
  }
  #[derive(Serialize)]
  struct RequestBody<'a> {
    digests: Vec<RequestDigest<'a>>,
  }

  let body = RequestBody {
    digests: digests
      .iter()
      .map(|d| RequestDigest {
        digest: &d.digest,
        size: d.size,
        executable: d.executable,
      })
      .collect(),
  };
  let body_json = serde_json::to_vec(&body)
    .map_err(|e| Error::from_reason(format!("cafs-writer serialize: {e}")))?;

  let url = format!("{}/v1/files", agent_url.trim_end_matches('/'));
  let agent = ureq::AgentBuilder::new()
    .timeout(std::time::Duration::from_secs(600))
    .build();
  let response = agent
    .post(&url)
    .set("Content-Type", "application/json")
    .set("Accept-Encoding", "gzip")
    .send_bytes(&body_json)
    .map_err(|e| Error::from_reason(format!("cafs-writer POST {url}: {e}")))?;

  // The agent always responds with Content-Encoding: gzip. Stream through
  // a gunzip decoder to avoid materializing the compressed response.
  let mut gunzipped = Vec::with_capacity(1024 * 1024);
  let mut reader = flate2::read::GzDecoder::new(response.into_reader());
  reader
    .read_to_end(&mut gunzipped)
    .map_err(|e| Error::from_reason(format!("cafs-writer gunzip: {e}")))?;

  write_payload_to_cafs(&store_dir, &gunzipped)
}

fn write_payload_to_cafs(store_dir: &str, bytes: &[u8]) -> Result<u32> {
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

// =======================================================================
// Streaming writer
// =======================================================================
//
// Incremental variant of `write_files`: the caller pushes bytes as they
// arrive off the wire, and the Rust side parses and writes each file
// the moment its full content is in the buffer. This overlaps the HTTP
// download with disk writes — the buffered `write_files` call has to
// wait for the full response to gunzip before any file can be written.
//
// The Linux io_uring backend remains batch-only for now; on Linux this
// class falls back to std::fs::write on a rayon-backed task pool, same
// as non-Linux platforms.

/// A streaming CAFS writer. Push chunks as they arrive; call `finish`
/// once to wait for all in-flight writes and get the total file count.
///
/// Not thread-safe; a single writer serves one /v1/files response.
#[napi]
pub struct CafsStreamWriter {
  state: std::sync::Mutex<StreamState>,
}

struct StreamState {
  files_dir: PathBuf,
  buffer: Vec<u8>,
  header_skipped: bool,
  parent_dirs_created: std::collections::HashSet<String>,
  // tx is dropped during finish() so the collector can observe EOF.
  // Each dispatched write task clones tx; when all tasks complete and
  // the owning tx is dropped, rx.iter() terminates.
  tx: Option<crossbeam_channel::Sender<std::result::Result<u32, String>>>,
  rx: crossbeam_channel::Receiver<std::result::Result<u32, String>>,
}

#[napi]
impl CafsStreamWriter {
  #[napi(constructor)]
  pub fn new(store_dir: String) -> Result<Self> {
    let files_dir = PathBuf::from(store_dir).join("files");
    let (tx, rx) = crossbeam_channel::unbounded();
    Ok(CafsStreamWriter {
      state: std::sync::Mutex::new(StreamState {
        files_dir,
        buffer: Vec::with_capacity(64 * 1024),
        header_skipped: false,
        parent_dirs_created: std::collections::HashSet::new(),
        tx: Some(tx),
        rx,
      }),
    })
  }

  /// Append a chunk of the gunzipped payload and dispatch any now-complete
  /// file entries to the write pool.
  #[napi]
  pub fn push(&self, chunk: Buffer) -> Result<()> {
    let mut s = self.state.lock().map_err(poisoned)?;
    s.buffer.extend_from_slice(&chunk);
    drain_buffer(&mut s).map_err(|e| Error::from_reason(format!("cafs-writer parse error: {e}")))
  }

  /// Signal end-of-stream; blocks until all dispatched writes have
  /// completed and returns the total number of files newly written.
  #[napi]
  pub fn finish(&self) -> Result<u32> {
    // Drop our Sender clone so rx.iter() terminates once all task clones
    // also go out of scope.
    let rx = {
      let mut s = self.state.lock().map_err(poisoned)?;
      // Buffer should be empty or just contain the end marker at this point.
      drain_buffer(&mut s)
        .map_err(|e| Error::from_reason(format!("cafs-writer parse error: {e}")))?;
      s.tx.take();
      s.rx.clone()
    };

    let mut total = 0u32;
    for result in rx.iter() {
      total += result.map_err(|e| Error::from_reason(format!("cafs-writer write error: {e}")))?;
    }
    Ok(total)
  }
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Error {
  Error::from_reason("cafs-writer: state mutex poisoned (worker panic?)")
}

// Parse as many complete entries as fit in the buffer, dispatching
// each to the write pool. Leaves any trailing incomplete bytes in the
// buffer for the next push.
fn drain_buffer(s: &mut StreamState) -> std::result::Result<(), String> {
  let mut pos = 0usize;

  // Skip the one-time JSON header on the very first bytes we see.
  if !s.header_skipped {
    if s.buffer.len() < 4 {
      return Ok(());
    }
    let json_len = u32::from_be_bytes([s.buffer[0], s.buffer[1], s.buffer[2], s.buffer[3]])
      as usize;
    if s.buffer.len() < 4 + json_len {
      return Ok(());
    }
    pos = 4 + json_len;
    s.header_skipped = true;
  }

  loop {
    if s.buffer.len() - pos < 64 {
      break;
    }
    // End marker: 64 zero bytes. Everything beyond is ignored.
    if s.buffer[pos..pos + 64].iter().all(|&b| b == 0) {
      pos += 64;
      break;
    }
    if s.buffer.len() - pos < 69 {
      break; // need digest + 4-byte size + 1-byte mode
    }
    let size = u32::from_be_bytes([
      s.buffer[pos + 64],
      s.buffer[pos + 65],
      s.buffer[pos + 66],
      s.buffer[pos + 67],
    ]) as usize;
    let entry_len = 69 + size;
    if s.buffer.len() - pos < entry_len {
      break; // content not yet fully arrived
    }
    let digest_hex = hex_encode(&s.buffer[pos..pos + 64]);
    let executable = (s.buffer[pos + 68] & 0x01) != 0;
    let content = s.buffer[pos + 69..pos + entry_len].to_vec();
    pos += entry_len;

    // Create the files/XX/ prefix once per unique prefix.
    let prefix = digest_hex[..2].to_string();
    if s.parent_dirs_created.insert(prefix.clone()) {
      std::fs::create_dir_all(s.files_dir.join(&prefix))
        .map_err(|e| format!("mkdir {prefix}: {e}"))?;
    }

    let files_dir = s.files_dir.clone();
    let tx = s.tx.as_ref().ok_or("push() after finish()")?.clone();
    // Use rayon's global thread pool — a fresh per-writer pool oversubscribes
    // the machine when multiple worker threads run batches in parallel.
    rayon::spawn(move || {
      let res = write_entry_owned(&files_dir, &digest_hex, executable, &content);
      let _ = tx.send(res);
    });
  }

  // Drop the consumed prefix of the buffer in one shot.
  if pos > 0 {
    s.buffer.drain(..pos);
  }
  Ok(())
}

fn write_entry_owned(
  files_dir: &std::path::Path,
  digest_hex: &str,
  executable: bool,
  content: &[u8],
) -> std::result::Result<u32, String> {
  let (prefix, suffix) = digest_hex.split_at(2);
  let filename = if executable {
    format!("{suffix}-exec")
  } else {
    suffix.to_string()
  };
  let path = files_dir.join(prefix).join(filename);
  let mode = if executable { 0o755 } else { 0o644 };

  let mut opts = std::fs::OpenOptions::new();
  opts.write(true).create_new(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    opts.mode(mode);
  }
  #[cfg(not(unix))]
  let _ = mode;

  match opts.open(&path) {
    Ok(mut f) => {
      use std::io::Write;
      f.write_all(content)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
      Ok(1)
    }
    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(0),
    Err(e) => Err(format!("open {}: {e}", path.display())),
  }
}
