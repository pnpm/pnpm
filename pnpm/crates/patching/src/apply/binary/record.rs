//! A `GIT binary patch` record checked against the blob ids of its
//! `index` line, the way `git apply` checks it.

use diffy::binary::{
    BinaryBlock,
    BinaryPatch,
};
use flate2::{
    Decompress,
    FlushDecompress,
    Status,
};
use ssri::{
    Algorithm,
    IntegrityOpts,
};

/// A binary record whose blocks decode and whose `index` line names the
/// full blob ids of both images.
pub(in crate::apply) struct BinaryRecord<'patch, 'text> {
    patch: &'patch BinaryPatch<'text>,
    old_blob_id: &'text str,
    new_blob_id: &'text str,
}

/// How a record relates to the file on disk.
pub(in crate::apply) enum BinaryDirection {
    /// The file is the recorded pre-image; writing this content applies
    /// the record.
    Forward(Vec<u8>),
    /// The file is the recorded post-image.
    AlreadyPatched,
}

impl<'patch, 'text> BinaryRecord<'patch, 'text> {
    /// Pair `patch` with the blob ids of the record it was parsed from.
    ///
    /// `patch_text` is the text `patch` was parsed from. Its blocks borrow
    /// from it, which locates the record's header.
    ///
    /// Fails when the header has no full `index` line, as `git apply`
    /// does, or when a block holds a zlib stream that ends early.
    pub(in crate::apply) fn new(
        patch_text: &'text str,
        patch: &'patch BinaryPatch<'text>,
    ) -> Result<Self, String> {
        let BinaryPatch::Full { forward, reverse } = patch else {
            return Err("the patch has no binary data".to_string());
        };
        check_stream_ends(forward)?;
        check_stream_ends(reverse)?;
        let (old_blob_id, new_blob_id) =
            full_index_line(patch_text, forward).ok_or("a binary patch needs a full index line")?;
        Ok(BinaryRecord { patch, old_blob_id, new_blob_id })
    }

    /// Whether `bytes` is the recorded pre-image.
    pub(in crate::apply) fn is_preimage(&self, bytes: &[u8]) -> bool {
        git_blob_id(bytes, self.old_blob_id) == self.old_blob_id
    }

    /// Whether `bytes` is the recorded pre-image or post-image.
    pub(in crate::apply) fn is_recorded_image(&self, bytes: &[u8]) -> bool {
        self.is_preimage(bytes) || git_blob_id(bytes, self.new_blob_id) == self.new_blob_id
    }

    /// The content the forward block creates from an empty pre-image.
    pub(in crate::apply) fn created(&self) -> Result<Vec<u8>, String> {
        let created = self.patch
            .apply(&[])
            .map_err(|error| error.to_string())?;
        if git_blob_id(&created, self.new_blob_id) != self.new_blob_id {
            return Err("the record's binary data does not produce its post-image".into());
        }
        Ok(created)
    }

    /// Decide what `bytes` is by its blob id.
    ///
    /// The blocks cannot decide it: a `literal` block ignores its input, and
    /// a `delta` block checks only lengths and copy ranges.
    pub(in crate::apply) fn direction(&self, bytes: &[u8]) -> Result<BinaryDirection, String> {
        if git_blob_id(bytes, self.new_blob_id) == self.new_blob_id {
            return Ok(BinaryDirection::AlreadyPatched);
        }
        if !self.is_preimage(bytes) {
            return Err("the file matches neither the record's pre-image nor its post-image".into());
        }
        let updated = self.patch.apply(bytes).map_err(|error| error.to_string())?;
        if git_blob_id(&updated, self.new_blob_id) != self.new_blob_id {
            return Err("the record's binary data does not produce its post-image".into());
        }
        Ok(BinaryDirection::Forward(updated))
    }
}

/// The blob id git gives `bytes`, in the object format `recorded_id` uses:
/// SHA-1 or, in a SHA-256 repository, SHA-256.
fn git_blob_id(bytes: &[u8], recorded_id: &str) -> String {
    let algorithm = match recorded_id.len() {
        SHA256_HEX_LEN => Algorithm::Sha256,
        _ => Algorithm::Sha1,
    };
    IntegrityOpts::new()
        .algorithm(algorithm)
        .chain(format!("blob {}\0", bytes.len()))
        .chain(bytes)
        .result()
        .to_hex()
        .1
}

/// Length of a full SHA-1 object id in hex.
const SHA1_HEX_LEN: usize = 40;

/// Length of a full SHA-256 object id in hex.
const SHA256_HEX_LEN: usize = 64;

/// The `<old>..<new>` ids of the `index` line in the header of the record
/// that holds `forward`.
fn full_index_line<'text>(
    patch_text: &'text str,
    forward: &BinaryBlock<'text>,
) -> Option<(&'text str, &'text str)> {
    let block_offset = forward.data.data
        .as_ptr()
        .addr()
        .checked_sub(patch_text.as_ptr().addr())?;
    let before_block = patch_text.get(..block_offset)?;
    let header = &before_block[before_block.rfind("diff --git ")?..];
    let ids = header
        .lines()
        .find_map(|line| line.strip_prefix("index "))?
        .split(' ')
        .next()?;
    let (old, new) = ids.split_once("..")?;
    let is_full = |id: &str| matches!(id.len(), SHA1_HEX_LEN | SHA256_HEX_LEN);
    (is_full(old) && is_full(new) && old.len() == new.len()).then_some((old, new))
}

/// Reject a block whose zlib stream ends before its end marker or
/// inflates to a size other than the one its header declares.
///
/// diffy 0.5.2 loops forever decoding a stream that ends early.
fn check_stream_ends(block: &BinaryBlock<'_>) -> Result<(), String> {
    let compressed = decode_base85_lines(block.data.data)?;
    let corrupt = |reason: &str| format!("corrupt binary data: {reason}");
    let mut inflater = Decompress::new(true);
    let mut scratch = [0u8; 8192];
    loop {
        let progress_before = (inflater.total_in(), inflater.total_out());
        let unread = usize::try_from(inflater.total_in())
            .ok()
            .and_then(|consumed| compressed.get(consumed..))
            .ok_or_else(|| corrupt("the stream is too long"))?;
        let status = inflater
            .decompress(unread, &mut scratch, FlushDecompress::Finish)
            .map_err(|error| corrupt(&error.to_string()))?;
        if inflater.total_out() > block.data.size {
            return Err(corrupt("the content is larger than its header says"));
        }
        match status {
            Status::StreamEnd if inflater.total_out() == block.data.size => return Ok(()),
            Status::StreamEnd => {
                return Err(corrupt("the content is smaller than its header says"));
            }
            Status::Ok | Status::BufError
                if (inflater.total_in(), inflater.total_out()) == progress_before =>
            {
                return Err(corrupt("the stream ends early"));
            }
            Status::Ok | Status::BufError => {}
        }
    }
}

/// Git's base85 alphabet.
const BASE85_ALPHABET: &[u8; 85] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

/// The digit of each byte in [`BASE85_ALPHABET`], `None` for other bytes.
const BASE85_DIGITS: [Option<u8>; 256] = {
    let mut digits = [None; 256];
    let mut digit = 0;
    while digit < BASE85_ALPHABET.len() {
        digits[BASE85_ALPHABET[digit] as usize] = Some(digit as u8);
        digit += 1;
    }
    digits
};

/// Decode the base85 lines of a block.
fn decode_base85_lines(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    for line in data.split(|&byte| byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if !line.is_empty() {
            decode_base85_line(line, &mut decoded).ok_or("corrupt binary data: invalid base85")?;
        }
    }
    Ok(decoded)
}

/// Append one decoded line to `decoded`. The first character gives the
/// decoded length: `A`-`Z` for 1-26, `a`-`z` for 27-52.
fn decode_base85_line(line: &[u8], decoded: &mut Vec<u8>) -> Option<()> {
    let (&length_char, encoded) = line.split_first()?;
    let length = match length_char {
        b'A'..=b'Z' => usize::from(length_char - b'A') + 1,
        b'a'..=b'z' => usize::from(length_char - b'a') + 27,
        _ => return None,
    };
    let start = decoded.len();
    for group in encoded.chunks(5) {
        decoded.extend(decode_base85_group(group)?);
    }
    (decoded.len() >= start + length).then(|| decoded.truncate(start + length))
}

/// Decode five base85 characters into four bytes.
fn decode_base85_group(group: &[u8]) -> Option<[u8; 4]> {
    let [_, _, _, _, _] = group else { return None };
    let mut value: u32 = 0;
    for &symbol in group {
        let digit = BASE85_DIGITS[usize::from(symbol)]?;
        value = value
            .checked_mul(85)?
            .checked_add(u32::from(digit))?;
    }
    Some(value.to_be_bytes())
}
