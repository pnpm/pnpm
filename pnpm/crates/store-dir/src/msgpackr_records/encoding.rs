use super::{
    CafsFileInfo, Diagnostic, Display, Error, HashMap, PackageFilesIndex, RECORD_DEF_EXT_TYPE,
    SLOT_HI, SLOT_LO, SideEffectsDiff, SmartDefault, Value, write_map_header, write_str,
};

/// Encode a [`PackageFilesIndex`] to msgpackr-records bytes that match
/// pnpm v11's wire format closely enough that `Packr({useRecords: true,
/// moreTypes: true}).unpack(bytes)` decodes to the same JS shape pnpm
/// produces itself.
///
/// ## Why not `rmp_serde::to_vec_named`?
///
/// `rmp_serde` emits plain `MessagePack` — every struct becomes a `fixmap`
/// / `map16` / `map32`. That's a perfectly valid `MessagePack` encoding,
/// but msgpackr with `useRecords: true` interprets *every* msgpack map
/// (no matter the nesting depth) as a JS `Map` object, including the
/// top-level `PackageFilesIndex`. pnpm's reader then does
/// `pkgIndex.files` (a property access) on what is actually a `Map`,
/// gets `undefined`, and crashes with `files is not iterable`.
///
/// pnpm itself sidesteps this because it packs the outer struct with
/// `useRecords: true`, which makes msgpackr emit a **record**: the
/// `d4 72 <slot>` fixext1 header followed by a field-name array and the
/// values. Records decode back as plain JS objects, while legitimate JS
/// `Map` values (pnpm's `files` / `sideEffects` / `added`) are still
/// encoded as msgpack maps and decode back as `Map`. The decoder can
/// tell the two apart because records are marked with the fixext1
/// envelope; plain maps aren't.
///
/// So to interop with pnpm, pacquet has to emit records for the Rust
/// `struct`s (object-shape on the pnpm side) and keep plain msgpack
/// maps for the Rust `HashMap`s (`Map`-shape on the pnpm side). That's
/// what this encoder does.
///
/// ## Slot allocation
///
/// Slot `0x40` is reserved for the top-level [`PackageFilesIndex`] —
/// one per row, always first in the stream. Inner slots in
/// `0x41..=0x7f` are allocated **lazily, in first-seen order, one per
/// distinct record shape** (where "shape" is the set of fields that
/// instance actually carries). A single Rust type may therefore span
/// multiple slots if different optional-field combinations show up in
/// the same row: a `CafsFileInfo` carrying `checkedAt` lands in one
/// slot and a `CafsFileInfo` without it lands in another. Same-shape
/// instances downstream collapse to a single bare-slot byte, which is
/// the record-compression win records exist for.
///
/// This is what msgpackr itself does for the same traversal and shape
/// set, so pacquet's output is **wire-compatible** with msgpackr (same
/// record schemas, same slot numbers, same value encodings) — pnpm's
/// reader reconstructs the same JS shape from both. Exact bytes can
/// still differ when Rust's `HashMap` iterates `files` / `sideEffects`
/// / `added` entries in a different order than msgpackr's JS `Map`
/// iteration, which is fine for correctness but worth keeping in mind
/// when diffing bytes against a pnpm-written reference row.
///
/// ## Optional-field handling
///
/// - **`PackageFilesIndex`**: `algo` and `files` are always emitted;
///   `requires_build`, `requires_prepare`, `manifest`, and `side_effects` are included
///   in the record schema only when `Some`. The `manifest`
///   ([`serde_json::Value`]) is encoded recursively, with every
///   nested JSON object record-encoded so a pnpm reader sees them as
///   JS `Object`s (which `manifest.bin` / `manifest.directories?.bin`
///   property access can reach) rather than plain msgpack maps
///   (which msgpackr decodes as JS `Map`s, leaving those property
///   reads `undefined`).
/// - **`CafsFileInfo`**: optional `checkedAt` is omitted from the
///   record schema entirely when `None` rather than written as `nil`,
///   so the presence of `checkedAt` determines the shape and thus
///   the slot. When `Some`, it's written as `float 64` (see
///   [`CafsFileInfo::checked_at`] for why — msgpackr reads `uint 64`
///   as `BigInt`, which crashes pnpm's `mtimeMs - (checkedAt ?? 0)`).
/// - **`SideEffectsDiff`**: `added`, `deleted`, and `remoteOrigin` are
///   optional; each field set gets its own slot on first use.
pub fn encode_package_files_index(index: &PackageFilesIndex) -> Result<Vec<u8>, EncodeError> {
    let mut state = EncodeState::default();
    let mut out = Vec::with_capacity(256);
    encode_pkg_files_index_value(&mut out, &mut state, index)?;
    Ok(out)
}

/// Error type of [`encode_package_files_index`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EncodeError {
    #[display(
        "Ran out of msgpackr record slots: encountered more than \
         {max} distinct record shapes (slot range is 0x41..=0x7f). \
         `CafsFileInfo` contributes at most 2 shapes and \
         `SideEffectsDiff` at most 8; the rest are allocated lazily \
         from `PackageFilesIndex.manifest`'s nested object shapes, \
         which in practice fit comfortably inside the remaining \
         range for a single tarball's manifest. Reaching this error \
         likely means the encoder is being reused for a payload it \
         wasn't designed for."
    )]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_MSGPACKR_RECORDS_OUT_OF_RECORD_SLOTS))]
    OutOfRecordSlots { max: usize },
}

/// Slot allocated to the top-level [`PackageFilesIndex`] record.
/// A single stream always has exactly one of these, so it gets the
/// base slot. Inner records (`CafsFileInfo`, `SideEffectsDiff`) are
/// allocated lazily from [`FIRST_INNER_SLOT`] upwards, one slot per
/// distinct shape — see [`EncodeState::allocate_slot`].
pub(super) const PKG_FILES_INDEX_SLOT: u8 = SLOT_LO;

// 0x40
pub(super) const FIRST_INNER_SLOT: u8 = SLOT_LO + 1;

// 0x41

/// Tracks which shapes have been defined and what slot each got.
/// Mirrors msgpackr's own strategy: when it sees a new record instance
/// whose field set differs from anything previously packed, it
/// allocates a new slot rather than redefining an existing one — so
/// same-shape instances downstream collapse to a single bare-slot byte
/// (the point of records), and mixed-shape streams still decode
/// correctly without per-instance re-defs.
///
/// Shape keys are small bitmasks over the optional fields of each
/// record type, see [`cafs_shape`] / [`side_effects_shape`]. Each type has
/// at most a handful of possible shapes (2 for `CafsFileInfo`, 8 for
/// `SideEffectsDiff`), so the 0x40..=0x7f slot range is vastly
/// over-provisioned for realistic workloads.
#[derive(SmartDefault)]
pub(super) struct EncodeState {
    /// Shape → slot for every `CafsFileInfo` shape seen so far. The
    /// index is the shape bitmask produced by [`cafs_shape`] (2
    /// possible values today). `None` = shape hasn't been emitted yet
    /// in this stream.
    pub(super) cafs_slots: [Option<u8>; 2],
    /// Same for `SideEffectsDiff`, indexed by [`side_effects_shape`]
    /// (8 possible values).
    pub(super) side_effects_slots: [Option<u8>; 8],
    /// Field-name vector → slot for every JSON-object shape seen so
    /// far inside a manifest value. Shape keys are owned `Vec<String>`
    /// because the field names are read from a borrowed
    /// `serde_json::Map` whose lifetime ends before the next encode
    /// call wants the lookup. msgpackr does the equivalent thing for
    /// arbitrary JS objects under `useRecords: true`; pacquet has to
    /// match so a pnpm reader sees the manifest's nested objects as JS
    /// `Object`s (record-decoded) rather than `Map`s (plain-msgpack-
    /// decoded), which is what pnpm's bin linker reads with
    /// `manifest.bin` / `manifest.directories?.bin` property access.
    pub(super) json_object_slots: HashMap<Vec<String>, u8>,
    /// Next unused slot in the 0x41..=0x7f range. Starts above
    /// [`PKG_FILES_INDEX_SLOT`] because the top-level record always
    /// takes slot 0x40.
    #[default(FIRST_INNER_SLOT)]
    pub(super) next_slot: u8,
}

impl EncodeState {
    pub(super) fn allocate_slot(&mut self) -> Result<u8, EncodeError> {
        if self.next_slot > SLOT_HI {
            return Err(EncodeError::OutOfRecordSlots {
                max: (SLOT_HI - FIRST_INNER_SLOT + 1) as usize,
            });
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        Ok(slot)
    }
}

/// Bitmask describing which optional fields a [`CafsFileInfo`] carries.
/// Bit 0 = `checked_at`. Required fields (digest, mode, size) don't
/// affect the shape because they're always present.
fn cafs_shape(info: &CafsFileInfo) -> u8 {
    u8::from(info.checked_at.is_some())
}

/// Bitmask describing which optional fields a [`SideEffectsDiff`]
/// carries. Bit 0 = `added`, bit 1 = `deleted`, bit 2 = `remoteOrigin`.
fn side_effects_shape(diff: &SideEffectsDiff) -> u8 {
    u8::from(diff.added.is_some())
        | (u8::from(diff.deleted.is_some()) << 1)
        | (u8::from(diff.remote_origin.is_some()) << 2)
}

fn encode_pkg_files_index_value(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    idx: &PackageFilesIndex,
) -> Result<(), EncodeError> {
    write_record_def_header(writer, PKG_FILES_INDEX_SLOT, &pkg_files_index_fields(idx));

    // Values in the same order as the field names above.
    write_str(writer, &idx.algo);
    if let Some(requires_build) = idx.requires_build {
        write_bool(writer, requires_build);
    }
    if let Some(requires_prepare) = idx.requires_prepare {
        write_bool(writer, requires_prepare);
    }
    if let Some(manifest) = &idx.manifest {
        encode_json_value(writer, state, manifest)?;
    }
    // Iterate the file map in sorted-key order so the emitted
    // msgpack bytes are byte-stable across runs. `HashMap`'s
    // iteration is randomised, which would make every row pacquet
    // writes appear "changed" on byte-diff even when the logical
    // content is identical. Sorting here matches what msgpackr-on-
    // JS effectively delivers via `Object` insertion order on
    // deterministic input (npm tarballs walk files in directory
    // order, which is sorted on most filesystems).
    write_map_header(writer, idx.files.len());
    for (name, info) in sorted_by_key(&idx.files) {
        write_str(writer, name);
        encode_cafs_file_info(writer, state, info)?;
    }
    if let Some(side_effects) = &idx.side_effects {
        write_map_header(writer, side_effects.len());
        for (platform, diff) in sorted_by_key(side_effects) {
            write_str(writer, platform);
            encode_side_effects_diff(writer, state, diff)?;
        }
    }
    if let Some(quarantine) = &idx.remote_side_effects_quarantine {
        write_map_header(writer, quarantine.len());
        for (channel, digests) in sorted_by_key(quarantine) {
            write_str(writer, channel);
            write_string_array(writer, digests);
        }
    }

    Ok(())
}

/// Field order `[algo, requiresBuild?, requiresPrepare?, manifest?, files,
/// sideEffects?, remoteSideEffectsQuarantine?]`. Optional fields are omitted
/// from the schema when `None`, matching msgpackr's field-omit-when-absent
/// shape so a pnpm reader sees the same JS object regardless of whether
/// pacquet or pnpm wrote the row.
fn pkg_files_index_fields(idx: &PackageFilesIndex) -> Vec<&'static str> {
    let mut fields: Vec<&str> = Vec::with_capacity(7);
    fields.push("algo");
    if idx.requires_build.is_some() {
        fields.push("requiresBuild");
    }
    if idx.requires_prepare.is_some() {
        fields.push("requiresPrepare");
    }
    if idx.manifest.is_some() {
        fields.push("manifest");
    }
    fields.push("files");
    if idx.side_effects.is_some() {
        fields.push("sideEffects");
    }
    if idx.remote_side_effects_quarantine.is_some() {
        fields.push("remoteSideEffectsQuarantine");
    }
    fields
}

fn write_string_array(writer: &mut Vec<u8>, values: &[String]) {
    write_array_header(writer, values.len());
    for value in values {
        write_str(writer, value);
    }
}

/// Sort a `HashMap` by key into a `Vec` of `(key, value)`
/// references. Used by the msgpackr-records encoder so every map
/// it writes — `PackageFilesIndex.files`, `…side_effects`,
/// `SideEffectsDiff.added` — comes out in lexicographic key
/// order. Without this the row payload depends on
/// `HashMap`'s randomised iteration and isn't reproducible.
fn sorted_by_key<Value>(map: &HashMap<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

/// Emit one JSON value as msgpack inside an active records stream.
/// Scalars use the smallest slot-safe encoding (no bare positive
/// fixints in `0x40..=0x7f`, which would otherwise be misread as
/// record-slot references — see [`write_uint`]). Arrays are plain
/// msgpack `fixarray` / `array16` / `array32`. Objects are
/// **record-encoded** via [`encode_json_object`] so that
/// `useRecords: true` decoders see them as JS `Object` rather than
/// JS `Map` — necessary for pnpm's bin linker to find
/// `manifest.bin` / `manifest.directories?.bin` via property
/// access.
fn encode_json_value(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    value: &Value,
) -> Result<(), EncodeError> {
    match value {
        Value::Null => writer.push(0xc0),
        Value::Bool(b) => write_bool(writer, *b),
        Value::Number(n) => encode_json_number(writer, n),
        Value::String(s) => write_str(writer, s),
        Value::Array(arr) => {
            write_array_header(writer, arr.len());
            for item in arr {
                encode_json_value(writer, state, item)?;
            }
        }
        Value::Object(obj) => encode_json_object(writer, state, obj)?,
    }
    Ok(())
}

/// Record-encode a JSON object: allocate one slot per distinct key
/// set seen in the current stream, emit a record def the first time
/// each shape appears, and emit a bare slot byte on subsequent
/// instances of the same shape. The slot table lives on
/// [`EncodeState::json_object_slots`] so reuse compresses repeated
/// nested-object shapes (e.g. multiple `bin: { command: path }`
/// objects with the same single command name) the same way
/// msgpackr's records mode does.
///
/// Field iteration order is the [`serde_json::Map`]'s own order;
/// pacquet builds with `serde_json/preserve_order`, so that's the
/// insertion order from parsing the original `package.json` — the
/// same order pnpm itself observes when packing the manifest.
fn encode_json_object(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    obj: &serde_json::Map<String, Value>,
) -> Result<(), EncodeError> {
    let fields: Vec<String> = obj.keys().cloned().collect();
    if let Some(&slot) = state.json_object_slots.get(&fields) {
        writer.push(slot);
    } else {
        let slot = state.allocate_slot()?;
        let field_refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        write_record_def_header(writer, slot, &field_refs);
        state.json_object_slots.insert(fields, slot);
    }
    for value in obj.values() {
        encode_json_value(writer, state, value)?;
    }
    Ok(())
}

/// Encode a [`serde_json::Number`] using the smallest slot-safe
/// `MessagePack` form. The branch order matches what pnpm's msgpackr
/// itself picks: any integer value first (so a JSON `1.0` parsed as
/// `Number(1)` stays an integer on the wire), falling through to
/// `float 64` only when the number genuinely needs the precision.
fn encode_json_number(writer: &mut Vec<u8>, n: &serde_json::Number) {
    if let Some(u) = n.as_u64() {
        write_uint(writer, u);
        return;
    }
    if let Some(i) = n.as_i64() {
        write_int(writer, i);
        return;
    }
    if let Some(f) = n.as_f64() {
        write_float64(writer, f);
    }
    // Unreachable: a `serde_json::Number` is always one of the three
    // cases above. If it isn't (a future serde_json release adds a
    // new representation), the wire output would be missing a value
    // for this field, which would surface as a deserialize error on
    // round-trip — louder than silent corruption.
}

fn encode_cafs_file_info(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    info: &CafsFileInfo,
) -> Result<(), EncodeError> {
    let shape = cafs_shape(info);
    if let Some(slot) = state.cafs_slots[shape as usize] {
        writer.push(slot); // bare slot = record reference; no def needed
    } else {
        let slot = state.allocate_slot()?;
        state.cafs_slots[shape as usize] = Some(slot);
        let fields: &[&str] = if info.checked_at.is_some() {
            &["digest", "mode", "size", "checkedAt"]
        } else {
            &["digest", "mode", "size"]
        };
        write_record_def_header(writer, slot, fields);
    }

    write_str(writer, &info.digest);
    write_uint(writer, u64::from(info.mode));
    write_uint(writer, info.size);
    if let Some(v) = info.checked_at {
        // Float 64 — not uint 64 — because msgpackr decodes `uint 64`
        // as a JS `BigInt`, and pnpm's integrity check does
        // `mtimeMs - (checkedAt ?? 0)` which throws `TypeError: Cannot
        // mix BigInt and other types`. Packing as a double matches
        // what pnpm writes for the same millisecond-epoch value (JS
        // Number is a double, so msgpackr emits `cb` + 8 bytes for
        // values past int32 range).
        write_float64(writer, v as f64);
    }
    Ok(())
}

fn encode_side_effects_diff(
    writer: &mut Vec<u8>,
    state: &mut EncodeState,
    diff: &SideEffectsDiff,
) -> Result<(), EncodeError> {
    let shape = side_effects_shape(diff);
    if let Some(slot) = state.side_effects_slots[shape as usize] {
        writer.push(slot);
    } else {
        let slot = state.allocate_slot()?;
        state.side_effects_slots[shape as usize] = Some(slot);
        write_record_def_header(writer, slot, &side_effects_fields(diff));
    }

    if let Some(added) = &diff.added {
        write_map_header(writer, added.len());
        for (name, info) in sorted_by_key(added) {
            write_str(writer, name);
            encode_cafs_file_info(writer, state, info)?;
        }
    }
    if let Some(deleted) = &diff.deleted {
        write_string_array(writer, deleted);
    }
    if let Some(origin) = &diff.remote_origin {
        let value = serde_json::to_value(origin)
            .expect("RemoteSideEffectsOrigin serialization cannot fail");
        encode_json_value(writer, state, &value)?;
    }
    Ok(())
}

/// Field order `[added?, deleted?, remoteOrigin?]`, omitting what the diff
/// does not carry.
fn side_effects_fields(diff: &SideEffectsDiff) -> Vec<&'static str> {
    let mut fields = Vec::with_capacity(3);
    if diff.added.is_some() {
        fields.push("added");
    }
    if diff.deleted.is_some() {
        fields.push("deleted");
    }
    if diff.remote_origin.is_some() {
        fields.push("remoteOrigin");
    }
    fields
}

/// `d4 72 <slot>` fixext1 header + msgpack array of `fields` as strings.
fn write_record_def_header(writer: &mut Vec<u8>, slot: u8, fields: &[&str]) {
    writer.push(0xd4);
    writer.push(RECORD_DEF_EXT_TYPE);
    writer.push(slot);
    write_array_header(writer, fields.len());
    for field in fields {
        write_str(writer, field);
    }
}

fn write_array_header(writer: &mut Vec<u8>, n: usize) {
    if n < 16 {
        writer.push(0x90 | (n as u8));
    } else if u16::try_from(n).is_ok() {
        writer.push(0xdc);
        writer.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        // `array 32` tops out at `u32::MAX` entries. Checked cast
        // so an overflow panics with a clear message rather than
        // silently truncating to a corrupt length prefix.
        let n = u32::try_from(n).expect("array length exceeds MessagePack's u32::MAX limit");
        writer.push(0xdd);
        writer.extend_from_slice(&n.to_be_bytes());
    }
}

/// Write an unsigned integer in the smallest `MessagePack` encoding that
/// is safe inside an active records stream. Values `0x40..=0x7f` cannot
/// be emitted as positive fixints — their byte representation collides
/// with record-slot references — so they get promoted to `uint 8`.
/// msgpackr does the same thing under `useRecords: true` for exactly
/// the same reason. `mode: u32` (e.g. `0o755` = 493) and `size: u64`
/// round-trip through this.
pub(super) fn write_uint(writer: &mut Vec<u8>, value: u64) {
    if value < u64::from(SLOT_LO) {
        // Positive fixint 0x00..=0x3f — below the slot range, safe to
        // emit bare.
        writer.push(value as u8);
    } else if u8::try_from(value).is_ok() {
        // Covers 0x40..=0xff; the 0x40..=0x7f sub-range must use uint 8
        // so the decoder doesn't mistake it for a slot byte.
        writer.push(0xcc);
        writer.push(value as u8);
    } else if u16::try_from(value).is_ok() {
        writer.push(0xcd);
        writer.extend_from_slice(&(value as u16).to_be_bytes());
    } else if u32::try_from(value).is_ok() {
        writer.push(0xce);
        writer.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        writer.push(0xcf);
        writer.extend_from_slice(&value.to_be_bytes());
    }
}

fn write_float64(writer: &mut Vec<u8>, value: f64) {
    writer.push(0xcb);
    writer.extend_from_slice(&value.to_be_bytes());
}

/// Write a signed integer in the smallest `MessagePack` encoding.
/// Negative values use the int 8/16/32/64 family (`0xd0..=0xd3`),
/// whose header bytes are outside the records-mode slot range so
/// they're always safe to emit. Non-negative values delegate to
/// [`write_uint`] which handles the slot-byte fixint promotion.
fn write_int(writer: &mut Vec<u8>, value: i64) {
    if value >= 0 {
        write_uint(writer, value as u64);
    } else if value >= -32 {
        // Negative fixint `0xe0..=0xff`; outside slot range.
        writer.push(value as i8 as u8);
    } else if value >= i64::from(i8::MIN) {
        writer.push(0xd0);
        writer.push(value as i8 as u8);
    } else if value >= i64::from(i16::MIN) {
        writer.push(0xd1);
        writer.extend_from_slice(&(value as i16).to_be_bytes());
    } else if value >= i64::from(i32::MIN) {
        writer.push(0xd2);
        writer.extend_from_slice(&(value as i32).to_be_bytes());
    } else {
        writer.push(0xd3);
        writer.extend_from_slice(&value.to_be_bytes());
    }
}

fn write_bool(writer: &mut Vec<u8>, value: bool) {
    writer.push(if value { 0xc3 } else { 0xc2 });
}
