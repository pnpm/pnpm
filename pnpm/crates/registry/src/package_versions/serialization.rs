//! How a [`PackageVersions`] map is read from and written to JSON.

use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    sync::Arc,
};

use deser::{
    Atom, Deserialize, Error, ErrorKind, Serialize, Source, State,
    de::{Sink, SinkHandle},
    ser::{Chunk, SerializeHandle},
};
use deser_value::Value;

use super::{FragmentSource, PackageVersions, VersionSlot};
use crate::json;

/// Captures every version as the byte range its value spans in the
/// document, which [`json::from_shared_str`] publishes as the
/// deserialization's [`Source`]. deser has no raw value: the parser still
/// walks every version, but none of them is built.
impl<'de> Deserialize<'de> for PackageVersions {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        let sink = VersionsSink { out, version: None, range: None, ranges: HashMap::new() };
        SinkHandle::arena(sink, state)
    }
}

struct VersionsSink<'out> {
    out: &'out mut Option<PackageVersions>,
    version: Option<String>,
    range: Option<Range<usize>>,
    ranges: HashMap<String, Range<usize>>,
}

impl VersionsSink<'_> {
    /// Record the version whose value just ended. A version listed twice
    /// keeps its last value, like the rest of the document.
    fn record_pending(&mut self) {
        if let (Some(version), Some(range)) = (self.version.take(), self.range.take()) {
            self.ranges.insert(version, range);
        }
    }
}

impl<'de> Sink<'de> for VersionsSink<'_> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.record_pending();
        Ok(String::deserialize_into(&mut self.version, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::arena(RangeSink { out: &mut self.range, start: None }, state))
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.record_pending();
        let document = state
            .get::<Source>()
            .map(|source| Arc::clone(&source.0))
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Unexpected,
                    "versions can only be decoded with their document",
                )
            })?;
        let slots = self.ranges
            .drain()
            .map(|(version, range)| {
                let source = FragmentSource::Shared { document: Arc::clone(&document), range };
                (version, VersionSlot::unparsed(source))
            })
            .collect();
        *self.out = Some(PackageVersions::from_slots(slots));
        Ok(())
    }
}

/// Records the byte range of one value and ignores its content.
struct RangeSink<'out> {
    out: &'out mut Option<Range<usize>>,
    start: Option<usize>,
}

impl RangeSink<'_> {
    fn begin(&mut self, state: &State) -> Result<(), Error> {
        self.start = Some(input_range(state)?.start);
        Ok(())
    }
}

impl<'de> Sink<'de> for RangeSink<'_> {
    fn atom(&mut self, _atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state)
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::null())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::null())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let end = input_range(state)?.end;
        *self.out = self.start.map(|start| start..end);
        Ok(())
    }
}

fn input_range(state: &State) -> Result<Range<usize>, Error> {
    state
        .input_range()
        .ok_or_else(|| Error::new(ErrorKind::Unexpected, "the format reports no input ranges"))
}

/// Fragments are written back as the values they hold, in the document's
/// key order. deser cannot splice JSON text into its output, so each
/// fragment is decoded into a [`Value`] first; the text that comes out is
/// equivalent but not necessarily byte-identical (escapes and number
/// spelling are normalized). File-span fragments read their span here
/// (rare: only a file-loaded packument being re-serialized).
impl Serialize for PackageVersions {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        let mut versions = BTreeMap::new();
        for (version, slot) in &self.slots {
            if let Some(value) = slot.value(version)? {
                versions.insert(version.as_str(), value);
            }
        }
        Ok(Chunk::Forward(SerializeHandle::arena(versions, state)))
    }
}

impl VersionSlot {
    /// The slot's manifest as a [`Value`], `None` for a slot with neither
    /// a readable fragment nor a typed manifest, which serializes as
    /// absent rather than failing the whole document.
    fn value(&self, version: &str) -> Result<Option<Value>, Error> {
        if let Some(json) = self.source.json() {
            return match json::from_str::<Value>(&json) {
                Ok(value) => Ok(Some(value)),
                Err(error) => {
                    tracing::warn!(
                        target: "pnpm_registry",
                        %error,
                        version,
                        "skipping registry version with a corrupt fragment during serialization",
                    );
                    Ok(None)
                }
            };
        }
        match self.parsed.get() {
            Some(Some(parsed)) => deser_value::to_value(parsed.as_ref()).map(Some),
            _ => Ok(None),
        }
    }
}
