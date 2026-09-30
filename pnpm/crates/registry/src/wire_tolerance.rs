//! Tolerant decoding for registry wire-format variance.
//!
//! The npm wire format is a de-facto contract rather than a specified
//! one, and registries mirroring npm diverge on the shape of individual
//! fields. That divergence is unusually expensive here: a version
//! manifest that fails to decode is skipped as though the registry never
//! published the version, so a single unmodeled field can erase
//! `dist-tags.latest` from a packument and strand resolution.
//!
//! Fields whose value pnpm does not depend on are therefore decoded
//! leniently — the field degrades to `None`, never the version. Fields
//! pnpm *does* depend on stay strict, because a version that cannot be
//! installed safely must fail loudly rather than quietly: see
//! [`PackageDistribution::integrity`](crate::PackageDistribution), where
//! an unusable value has to reach the install as an error.
//!
//! Each decoder is a type that is built from the field's value as a
//! [`Value`] and converts into the field's type, and a field selects it
//! with `#[deser(default, deserialize_as = FromInto<Decoder>)]`.

use std::collections::HashMap;

use deser::{
    State,
    adapters::{DeserializeAs, FromInto, TryFromInto},
    de::{Deserialize, DeserializeOwned, SinkHandle},
};
use deser_value::Value;

/// Create the sink of a decoder: the value is decoded into a [`Value`]
/// first and converted with the decoder's `From<Value>` (through
/// `Adapter = FromInto<Value>`) or `TryFrom<Value>` (through
/// `Adapter = TryFromInto<Value>`).
fn decode_through_value<'out, 'de, Adapter, Decoder>(
    out: &'out mut Option<Decoder>,
    state: &mut State,
) -> SinkHandle<'out, 'de>
where
    Adapter: DeserializeAs<'de, Decoder>,
{
    Adapter::deserialize_into_as(out, state)
}

/// A field pnpm reads for presence alone. The typed body is kept when the
/// registry sends one, and any other truthy shape decodes as
/// present-with-no-detail.
///
/// npm serves these markers as objects, but their body is not part of
/// the wire contract that registries mirroring npm honor — some
/// abbreviate a marker to a bare flag such as `1`. Nothing downstream
/// reads the body, only whether the marker is there, so an unrecognized
/// one must not cost the version.
///
/// Presence is decided by JavaScript truthiness, not by the field merely
/// being set: these markers rank supply-chain trust evidence, and a value
/// the TypeScript resolver reads as no evidence must not read as evidence
/// here.
pub(crate) struct PresenceMarker<Marker>(Option<Marker>);

impl<'de, Marker: DeserializeOwned + Default + Send + 'static> Deserialize<'de>
    for PresenceMarker<Marker>
{
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<FromInto<Value>, _>(out, state)
    }
}

impl<Marker: DeserializeOwned + Default> From<Value> for PresenceMarker<Marker> {
    fn from(value: Value) -> Self {
        if !is_truthy(&value) {
            return PresenceMarker(None);
        }
        PresenceMarker(Some(deser_value::from_value(&value).unwrap_or_default()))
    }
}

impl<Marker> From<PresenceMarker<Marker>> for Option<Marker> {
    fn from(marker: PresenceMarker<Marker>) -> Self {
        marker.0
    }
}

/// A record whose fields pnpm reads, tolerating a registry that sends
/// something other than an object in its place.
///
/// Unlike [`PresenceMarker`], the container's *presence* carries no
/// signal of its own — everything pnpm wants is inside it — so a
/// non-object decodes as absent rather than as an empty record.
pub(crate) struct RecordOrAbsent<Record>(Option<Record>);

impl<'de, Record: DeserializeOwned + Send + 'static> Deserialize<'de> for RecordOrAbsent<Record> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<FromInto<Value>, _>(out, state)
    }
}

impl<Record: DeserializeOwned> From<Value> for RecordOrAbsent<Record> {
    fn from(value: Value) -> Self {
        if !value.is_map() {
            return RecordOrAbsent(None);
        }
        RecordOrAbsent(deser_value::from_value(&value).ok())
    }
}

impl<Record> From<RecordOrAbsent<Record>> for Option<Record> {
    fn from(record: RecordOrAbsent<Record>) -> Self {
        record.0
    }
}

/// A map of records, tolerating an entry whose value is not an object as
/// well as a container that is not a map.
///
/// The keys carry the signal — the TypeScript resolver reads
/// `peerDependenciesMeta` by name to learn which peers a manifest
/// declares — so a non-object entry keeps its key with a default record
/// rather than costing the version. A non-object container decodes as
/// absent, like [`RecordOrAbsent`].
pub(crate) struct RecordMap<Record>(Option<HashMap<String, Record>>);

impl<'de, Record: DeserializeOwned + Default + Send + 'static> Deserialize<'de>
    for RecordMap<Record>
{
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<FromInto<Value>, _>(out, state)
    }
}

impl<Record: DeserializeOwned + Default> From<Value> for RecordMap<Record> {
    fn from(value: Value) -> Self {
        let Some(entries) = value.as_map() else { return RecordMap(None) };
        RecordMap(Some(
            entries
                .iter()
                .filter_map(|(name, record)| {
                    let name = name.as_str()?.to_owned();
                    Some((name, deser_value::from_value(record).unwrap_or_default()))
                })
                .collect(),
        ))
    }
}

impl<Record> From<RecordMap<Record>> for Option<HashMap<String, Record>> {
    fn from(map: RecordMap<Record>) -> Self {
        map.0
    }
}

/// A descriptive string pnpm carries but never acts on, tolerating a
/// registry that sends another scalar in its place.
///
/// These fields sit in the same record as the trust markers, and
/// [`RecordOrAbsent`] decodes that record as a unit: a strict decode here
/// would take a valid `approver` or `trustedPublisher` down with a
/// mistyped display name, ranking the version *below* what the
/// TypeScript resolver ranks it — which reads the markers without regard
/// to the shape of their siblings.
pub(crate) struct TextOrAbsent(Option<String>);

impl<'de> Deserialize<'de> for TextOrAbsent {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<FromInto<Value>, _>(out, state)
    }
}

impl From<Value> for TextOrAbsent {
    fn from(value: Value) -> Self {
        TextOrAbsent(
            value
                .is_str()
                .then(|| value.as_str().map(str::to_owned))
                .flatten(),
        )
    }
}

impl From<TextOrAbsent> for Option<String> {
    fn from(text: TextOrAbsent) -> Self {
        text.0
    }
}

/// A byte/entry count the resolver treats as advisory, accepting the
/// numeric shapes a registry's serializer may emit.
///
/// A JSON number that is integral and non-negative decodes whatever its
/// encoding — `12345` and `12345.0` are the same count, and a registry
/// whose backend round-trips through a float (Go's `float64`, Python's
/// `json`, Ruby's `JSON`) emits the latter. A numeric string is accepted
/// on the same reasoning. Anything else decodes as absent, which is
/// already a shape every reader handles.
pub(crate) struct AdvisoryCount(Option<usize>);

impl<'de> Deserialize<'de> for AdvisoryCount {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<FromInto<Value>, _>(out, state)
    }
}

impl From<Value> for AdvisoryCount {
    fn from(value: Value) -> Self {
        if let Some(text) = value.as_str() {
            return AdvisoryCount(text.trim().parse().ok());
        }
        AdvisoryCount(integral_count(&value))
    }
}

impl From<AdvisoryCount> for Option<usize> {
    fn from(count: AdvisoryCount) -> Self {
        count.0
    }
}

/// A float only counts when it is a whole number below 2^53, the range
/// in which `f64` holds every integer. Past that the JSON parser has
/// already rounded the digits it was given, and casting straight across
/// would saturate, turning a bogus `1e100` into `usize::MAX` — a size
/// the extractor would then try to honor.
fn integral_count(number: &Value) -> Option<usize> {
    if let Some(exact) = number.as_u64() {
        return usize::try_from(exact).ok();
    }
    if number.as_i64().is_some() {
        return None;
    }
    const EXACT_FLOAT_LIMIT: f64 = (1u64 << 53) as f64;
    let float = number.as_f64()?;
    ((0.0..EXACT_FLOAT_LIMIT).contains(&float) && float.fract() == 0.0)
        .then_some(float as u64)
        .and_then(|count| usize::try_from(count).ok())
}

/// A flag that only counts when the registry sends a real boolean.
///
/// The TypeScript resolver compares these with `=== true`, so a string
/// `"true"` is not a `true` there and must not become one here. Decoding
/// every non-boolean as absent reproduces that comparison exactly while
/// keeping an off-shape value from costing the whole version.
pub(crate) struct StrictFlag(Option<bool>);

impl<'de> Deserialize<'de> for StrictFlag {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<FromInto<Value>, _>(out, state)
    }
}

impl From<Value> for StrictFlag {
    fn from(value: Value) -> Self {
        StrictFlag(value.as_bool())
    }
}

impl From<StrictFlag> for Option<bool> {
    fn from(flag: StrictFlag) -> Self {
        flag.0
    }
}

/// The `deprecated` field of a version manifest.
///
/// The field is nominally a string, but the real npm registry
/// occasionally serves `"deprecated": false` for never-deprecated
/// versions. A `false` (and `null`) decodes as absent, a `true` as a
/// deprecation without a reason. Any other shape fails the manifest.
pub(crate) struct DeprecationReason(Option<String>);

impl<'de> Deserialize<'de> for DeprecationReason {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        decode_through_value::<TryFromInto<Value>, _>(out, state)
    }
}

impl TryFrom<Value> for DeprecationReason {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        if value.is_null() {
            return Ok(DeprecationReason(None));
        }
        if let Some(flag) = value.as_bool() {
            return Ok(DeprecationReason(flag.then(String::new)));
        }
        match value.as_str() {
            Some(reason) if value.is_str() => Ok(DeprecationReason(Some(reason.to_owned()))),
            _ => Err("expected a deprecation reason (string), a boolean, or null"),
        }
    }
}

impl From<DeprecationReason> for Option<String> {
    fn from(reason: DeprecationReason) -> Self {
        reason.0
    }
}

/// JavaScript truthiness of a decoded JSON value.
fn is_truthy(value: &Value) -> bool {
    if value.is_null() {
        return false;
    }
    if let Some(flag) = value.as_bool() {
        return flag;
    }
    if value.is_str() {
        return value
            .as_str()
            .is_some_and(|text| !text.is_empty());
    }
    if value.is_seq() || value.is_map() {
        return true;
    }
    value
        .as_f64()
        .is_some_and(|number| number != 0.0)
}

#[cfg(test)]
mod tests;
