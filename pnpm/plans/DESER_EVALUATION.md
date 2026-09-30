# Evaluating deser as a replacement for serde

This note records an evaluation of [deser](https://github.com/mitsuhiko/deser)
0.9.1 ([announcement](https://lucumr.pocoo.org/2026/9/29/deser/),
[API docs](https://docs.rs/deser)) as a replacement for serde in pnpm v12. It
covers how deser is built, what porting pnpm's two hottest decoding paths
took, what the ports measured, and whether switching is worth it.

## Verdict

**Do not switch.** deser reads YAML about twice as fast as serde-saphyr,
but it reads JSON slower than serde_json, and JSON is on pnpm's hottest
path. With the registry ported, parsing a packument takes 1.44x as long
and decoding every version of one 1.28x as long (geometric means over 15
and 16 real packuments). End to end, resolving the integrated benchmark's
fixture offline from a warm metadata cache takes 7 to 8% longer.

The cause is structural. pnpm parses a packument lazily: serde_json's
`RawValue` lets it pass over every version manifest without building
anything, and decode only the versions the resolver picks. deser has no
raw value and no fast path for skipping. Every skipped value still goes
through its event machinery, so the lazy parse, the part serde_json makes
cheapest, takes 1.84x as long.

The lockfile gain is real but small in practice. With the lockfile reader
ported, loading a lockfile takes about half as long (24.9 ms to 12.9 ms
for a 376 KiB lockfile). pnpm reads it once per install, and in installs
that link packages the saving disappears into filesystem work. No linking
scenario of the integrated benchmark moved outside its noise.

deser's other strengths (no stack overflows, lossless buffering, `flatten`
without buffering, built-in adapters, structured errors) are real. They
do not fix a problem pnpm has today: pnpm already caps nesting and already
works around the serde limitations it runs into. Against that stand a
migration that has to move whole type graphs at once, a `serde_json::Value`
boundary through the codebase, and a young dependency.

## What deser is

deser is a serialization framework by Armin Ronacher. Its design answers
problems that serde's users keep running into: the repository's
`SERDE.md` lists, for each design choice, the serde issues behind it. It
ships as a family of crates:

| crate | role |
|---|---|
| `deser` | facade: traits, derive, adapters |
| `deser-core` | the data model, sinks, emitters, drivers, layers, arena |
| `deser-derive` | `#[derive(Serialize, Deserialize)]` |
| `deser-json`, `deser-yaml`, `deser-toml`, `deser-cbor`, `deser-msgpack`, ... | formats |
| `deser-value` | a dynamic value (`IndexMap` based) |
| `deser-serde` | the `Serde` adapter, to use serde types inside deser types |

### Push instead of pull

serde is pull-based. A type's `Deserialize` impl asks the format for what it
expects (`deserialize_map`, `deserialize_str`, ...) and the format answers
through a `Visitor`. Each nested value is a nested call, so nesting uses the
call stack, and every `Deserialize` impl is monomorphized for every format
it is used with.

deser is push-based. The format parses the input into events (atoms, map
and sequence starts, keys, values, ends) and pushes them into a `Sink` that
the type creates. A sink that needs a nested value returns the nested sink
as a `SinkHandle` to a driver, and the driver keeps the chain of sinks on
the heap. Sinks are trait objects allocated in an arena. Serialization
mirrors this with emitters and chunks (`Chunk::Forward` hands a nested
value back to the driver).

Consequences:

- Nesting never uses the call stack. A million levels deserialize,
  serialize, and skip without overflowing, and `deser_value::Value` drops,
  clones, and compares without recursion.
- Everything goes through dynamic dispatch. Derived code is small and
  compiles faster (the project measures release builds of derived code at
  about 1.6x the speed of serde's), but every event pays for a virtual call.
- Only self-describing formats are supported. bincode-style formats cannot
  be driven by events.
- Sinks are `Send` and types are driven by events, so a deserialization
  can pause mid-document and resume on another thread (streaming JSON,
  CBOR, MessagePack).

### A richer data model

serde's data model is a fixed set of Rust-like types. deser's atoms carry
what the format knows:

- `Atom::Lexical` is text whose type the format cannot express, such as a
  JSON object key or a query-string value. A sink for a number parses it.
- `Atom::Implicit` is a YAML plain scalar: the resolved value (`true`, `42`,
  `~`) together with its original text. A `String` field receiving
  `lockfileVersion: 9.0` gets `"9.0"`, not `"9"`.
- Extension values (128-bit integers, exact decimals, dates) carry a
  fallback atom for sinks that do not know them.

### Buffering and flattening

serde buffers internally tagged and untagged enums and `#[serde(flatten)]`
through its private `Content` type, which drops everything outside the data
model (numeric map keys, 128-bit integers, `arbitrary_precision` numbers,
source locations). deser records buffered values as events together with
the state the format published for them, and replays them losslessly.
`flatten` needs no buffer at all: a struct offers each key it does not know
to its flattened fields through `Sink::value_for_key`.

### Attributes, adapters, and state

- Attribute arguments are Rust, not strings:
  `#[deser(deserialize_as = Option<FromInto<Decoder>>)]`.
- Adapters in the style of `serde_with` are built in: `DisplayFromStr`,
  `FromInto`, `TryFromInto`, `DefaultOnError`, `MapSkipError`, `As`, and
  they compose with containers.
- A deserialization carries a `State` with typed values: the duplicate-key
  policy (`DuplicateKeys`, rejecting repeats by default), the document as a
  `Source` with each value's byte range, error context.
- Layers wrap the event stream. `Limits` caps depth and size.
- Errors have a kind, a position, a source error, and typed attachments.

### Limits of the design

From the code and from `LIMITATIONS.md`:

- There is no raw value and no fast path for skipping. An ignored value is
  still tokenized and dispatched event by event.
- Layers run on every event of a document.
- `deser-serde` goes one way: a serde type can sit inside a deser type
  (buffered through `Serde`), but a deser type cannot sit inside a serde
  type.
- A flattened field cannot take an adapter (`as`, `deserialize_as`) or
  `default`.
- The drivers and the arena use `unsafe` to erase lifetimes. It is
  documented and runs under miri, but it is more `unsafe` than serde needs.
- The ecosystem is young: version 0.9, one maintainer, and third-party
  types (`ssri::Integrity`, `node_semver::Version`, `url::Url`, `IndexMap`
  behind a feature) need adapters or the serde bridge.

## What was ported

The port is on this branch, in commits that each build and pass their tests.

1. `b59b1a8`: the micro-benchmark gained real-world inputs:
   `PNPM_MICRO_BENCHMARK_PACKUMENTS` points it at packuments on disk for
   `packument/parse/<name>` and `packument/hydrate_all/<name>`, and
   `lockfile/serialize_pnpm_lock` and `lockfile/parse_monorepo_lock` measure
   the lockfile.
2. `e4fcb35`, **registry metadata** (`pnpm-registry` and its users in the resolver, the
   CLI, and the package manager): packuments, version manifests, and the
   metadata mirror decode and encode with deser-json. Each version is kept
   as a byte range of the one shared document (the document is the
   deserialization's `Source`) instead of a `RawValue`.
3. `c36912a`, **depth cap at the boundary**: the registry decodes without `Limits`,
   and the new `pnpm-json-bridge` crate caps nesting where a
   `deser_value::Value` is converted into a `serde_json::Value`, the step
   that recurses.
4. `8144e52`, **lockfile reading** (`pnpm-lockfile`): `pnpm-lock.yaml` and the env
   lockfile are read with deser-yaml. The types derive both serde's and
   deser's `Deserialize`, because the lockfile writer and the repair loader
   still go through serde. Every lockfile test decodes its YAML with both
   and requires them to agree.

Not ported: the lockfile writer, `pnpm-workspace.yaml`, `.modules.yaml`,
package manifests, and the other ~20 crates that use serde-saphyr or
serde_json. The ported paths are the ones that run per package or per
install.

### What the port ran into

- **Duplicate keys.** deser rejects a repeated key by default. JavaScript's
  `JSON.parse`, and so npm and pnpm v11, keep the last value. The registry
  sets `DuplicateKeys::Last`.
- **Lenient decoders.** pnpm's registry types use `deserialize_with`
  functions that tolerate off-shape fields. Each became a decoder type
  selected with `deserialize_as = FromInto<Decoder>`, which decodes into a
  `Value` first.
- **`serde_json::Value` everywhere.** Manifests travel through pnpm as
  `serde_json::Value`. Code on either side of the port converts at the
  boundary with `to_serde_json` and `from_serde_json`, and the conversion
  is where the depth cap has to live.
- **One-way bridge.** Because a deser type cannot sit inside a serde type,
  a type decoded with deser needs every field type to implement deser's
  traits too. Porting the lockfile reader meant adding deser impls to every
  lockfile type while keeping the serde ones for the writer. A full
  migration has to move whole type graphs at once.
- **`flatten` with adapters.** `Lockfile::extra` flattens unknown top-level
  keys into `serde_json::Value`s. A flattened field cannot take an adapter,
  so `LockfileExtra` became a newtype with its own `Deserialize` impl.
- **Third-party types.** `ssri::Integrity` and the lockfile's string
  newtypes decode through `DisplayFromStr`. `IndexMap` needs deser's
  `indexmap` feature.
- **Error messages.** deser's messages differ from serde-saphyr's. The
  lockfile loader keeps its `message (line:column)` format.

## Measurements

All numbers come from the same 4-core cloud container. Micro-benchmark
numbers are criterion's point estimates from release builds, taken with
the container otherwise idle and the binaries run back to back.

### Standalone decoding, serde vs deser

A standalone harness decoded the same inputs both ways, alternating the two
sides and reporting medians and allocations. The inputs are 15 abbreviated
packuments from the npm registry (49 KiB for `chalk` to 24 MiB for `next`),
their full packuments, and four real lockfiles (376 KiB to 825 KiB).
"typed" decodes into structs shaped like pnpm's, "lazy" keeps each version
raw, "hydrate" decodes every version from its fragment.

| case | deser time / serde time (geometric mean) |
|---|---:|
| JSON into a dynamic value | 1.12x |
| JSON lazy (serde_json `RawValue` vs deser byte ranges) | 1.84x |
| JSON typed | 0.94x |
| JSON, hydrate every version | 1.17x |
| JSON serialize | 1.41x |
| YAML into a dynamic value | 0.42x |
| YAML typed (lockfile) | 0.36x |
| YAML serialize | 0.71x |

deser allocates fewer bytes in the typed cases (for `next`: 155 MiB
against 210 MiB, and for the largest lockfile: 3.7 MiB against 17.4 MiB),
but slightly more individual allocations. pnpm writes lockfiles with its
own emitter, not serde-saphyr, so the YAML serialize row does not apply.

### Registry port, in-repo micro-benchmark

`packument/parse/<name>` is the resolver's lazy parse,
`packument/hydrate_all/<name>` also decodes every version. The inputs are
the abbreviated packuments above.

| group | with `Limits` | without `Limits` |
|---|---:|---:|
| `packument/parse` (16 packuments) | 2.59x | 1.44x |
| `packument/hydrate_all` (15 packuments) | 1.70x | 1.28x |
| `lockfile` (3) | 1.01x | 0.99x |
| `version_pick` (2) | 1.02x | 1.02x |
| `workspace_resolution`, `workspace_sort` | 1.03x, 0.98x | 0.95x, 0.97x |

The second run reversed the order of the two binaries. The groups the port
does not touch stay within about 5%, which is the noise floor here.

What the decoding rules cost on their own, measured on plain deser-json
with pnpm's version type:

| setting | lodash | typescript | react |
|---|---:|---:|---:|
| `exact_numbers(false)` | 1.01x | 1.01x | 1.01x |
| plus `DuplicateKeys::Last` | 1.03x | 1.04x | 1.02x |
| plus `Limits::max_depth(128)` | 1.42x | 1.22x | 1.41x |
| lazy parse with all of the above | 1.82x | 2.04x | 1.79x |

A layer costs between a fifth and a half of the decoding time, which is
why the port caps depth during the conversion instead.

### Lockfile port, in-repo micro-benchmark

`lockfile/parse_pnpm_lock` loads a 376 KiB, 12,000-line lockfile from
`pnpm-testing-utils`, and `lockfile/parse_monorepo_lock` loads this
repository's own 830 KiB lockfile (an env document and a main document).
Both go through `Lockfile::load_wanted_from_dir`, file read included. The
binaries ran in the order deser, serde, serde, deser.

| benchmark | serde-saphyr | deser-yaml | deser/serde |
|---|---:|---:|---:|
| `lockfile/parse_pnpm_lock` | 24.82 ms, 24.98 ms | 12.80 ms, 13.00 ms | 0.52x |
| `lockfile/parse_monorepo_lock` | 47.73 ms, 46.90 ms | 21.55 ms, 21.77 ms | 0.46x |
| `lockfile/serialize_pnpm_lock` | 7.21 ms, 7.43 ms | 7.21 ms, 7.18 ms | 0.99x |

Reading a lockfile takes about half as long. Writing is unchanged, since the
writer was not ported. The standalone harness measured the bare decode at
0.36x, so the lockfile's own types (the string newtypes parsed through
`FromStr`, the `serde_json` fields) and the file read take part of the
gain back. The other groups moved by no more than their run-to-run noise.

### End to end, integrated benchmark

`integrated-benchmark` times real installs of its fixture project (a
12,000-line lockfile's worth of npm packages) with hyperfine, against a
local pnpr proxy of the npm registry, without latency or bandwidth caps.
Both `pnpm` binaries were built with the settings CI uses for this
benchmark (thin LTO, 16 codegen units): the serde baseline at `b59b1a8`
and the port with both halves at `8144e52`. Scenarios with two rows ran
twice, once in each target order.

| scenario | dominated by | serde | deser | deser/serde |
|---|---|---:|---:|---:|
| `fresh-resolve.hot-cache.offline` | packument parsing | 444.0 ± 6.4 ms | 475.9 ± 9.5 ms | 1.07x |
| | | 449.9 ± 11.9 ms | 486.6 ± 17.4 ms | 1.08x |
| `fresh-install.hot-cache.hot-store` | resolution and linking | 3.428 ± 0.075 s | 3.469 ± 0.157 s | 1.01x |
| `fresh-restore.hot-cache.hot-store` | lockfile and linking | 2.860 ± 0.143 s | 2.550 ± 0.439 s | 0.89x |
| | | 2.960 ± 0.122 s | 2.909 ± 0.116 s | 0.98x |
| `repeat-install.hot-cache.hot-store` | the up-to-date check | 20.5 ± 1.2 ms | 20.7 ± 1.3 ms | 1.01x |

The offline resolve is the scenario where packument parsing dominates, and
its user CPU time grew by 10 to 16% (432.8 ms to 475.1 ms, 426.7 ms to
493.7 ms). The installs that link spend 8 to 10 s of system time across
the cores on the filesystem, which hides both the slower packument parse
and the faster lockfile read. The frozen restore reads the lockfile, but
its first run's deser samples spread from 1.9 s to 3.2 s, and the second
run shows a tie. The repeat install finishes in 20 ms, less than parsing
the fixture's lockfile would take, so the up-to-date check does not parse
it in full.

### Reproducing

- Micro-benchmark: build `pnpm-micro-benchmark` in release mode at each
  revision, set `PNPM_MICRO_BENCHMARK_PACKUMENTS` to a directory of
  packument JSON files (named `<name>.json`, with `__` for `/`), and
  compare the two runs' criterion estimates.
- Integrated benchmark: build `pnpm` (and `pnpr` once) in release mode with
  `CARGO_PROFILE_RELEASE_LTO=thin` and
  `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, copy each binary to
  `<work-env>/pacquet@<rev>/pacquet/target/release/pnpm`, and run
  `integrated-benchmark --reuse-prebuilt-binaries --registry=verdaccio
  --scenario=<scenario> --work-env=<work-env> pacquet@<rev> pacquet@<rev>`.

### Binary size

The micro-benchmark binary grows from 15,954,512 to 15,993,904 bytes with
the registry port (+0.25%). With the lockfile port it drops to
14,841,784 bytes, 7% below the serde build. serde-saphyr is still linked
(`pnpm-config` uses it), but it no longer instantiates its decoder for
every lockfile type. The release `pnpm` binary, built with the thin LTO
settings CI's integrated benchmark uses, goes from 78,114,944 to
77,839,968 bytes (-0.35%).

## What pnpm would gain

- **Faster lockfile reads.** Loading a lockfile takes about half as
  long (24.9 ms to 12.9 ms for a 376 KiB lockfile, 47.3 ms to 21.7 ms for
  this repository's 830 KiB one). The standalone decode of the same
  documents is 2.8x as fast.
- **No stack overflows, and no budget tuning.** pnpm raises five
  serde-saphyr budgets to the document size so large lockfiles parse, and
  keeps a depth cap against hostile ones. deser needs neither for decoding.
  Only the conversion into recursive `serde_json` trees needs a cap.
- **`flatten` without buffering.** `Lockfile::extra` is flattened. With
  serde, that makes serde-saphyr buffer the lockfile's unknown keys through
  `Content`. deser offers each unknown key to the flattened field directly.
- **Lossless untagged enums.** Lockfile resolutions are an untagged enum.
  serde buffers the value through `Content`, which keeps only what serde's
  data model holds. deser replays the recorded events, YAML scalar text
  included.
- **A simpler lockfile writer, eventually.** The writer lowers the lockfile
  to a `serde_json::Value` and parallelizes the large maps through a
  thread-local stash. deser's emitters hand nested values back to the
  driver (`Chunk::Forward`), which would allow a direct emitter. This was
  not attempted.
- **Better errors.** Error kinds, positions, and attachments such as the
  path of the failing field.
- **Compile times.** Derived deser code is smaller. It could not be
  measured here because the ported types derive both.

## What pnpm would pay

- **Slower resolution.** The packument parse is 1.44x as slow and decoding
  versions 1.28x. Getting back to serde_json's lazy parse needs a raw value
  or a skip fast path in deser.
- **A long, all-or-nothing migration.** The one-way bridge means a type
  cannot move to deser until all of its field types can. During the
  migration, types derive both, and every change to a type has to keep two
  sets of attributes in sync. The lockfile reader needed deser impls on 39
  types and a test helper that checks the two decoders agree.
- **A `serde_json::Value` boundary.** Manifests are `serde_json::Value`
  throughout pnpm. Until they move, every crossing converts a tree, and
  the conversion has to cap depth itself.
- **Behavior to re-verify.** Duplicate keys, number handling, error text,
  and how YAML plain scalars resolve all differ in details. Each port has
  to pin them down again.
- **A young dependency.** deser is 0.9 with one maintainer and uses more
  `unsafe` than serde. The port builds six new crates. `Cargo.lock` also
  records deser-core's optional dependencies (`bigdecimal`,
  `rust_decimal`, `borsh`, and theirs), which pnpm does not build but
  supply-chain review still sees.

## If pnpm revisits this

- Ask upstream for a raw-value type or a skip fast path in deser-json. That
  is the one change that would make the JSON path competitive.
- Reading lockfiles with deser-yaml is the part worth keeping in mind. If
  lockfile parsing shows up in a profile (a very large monorepo, or
  commands that read the lockfile and do little else, like `pnpm list` or
  `pnpm why`), the reader can move on its own: the lockfile types can
  derive both, and the dual-decoder test helper keeps the two in step. It
  does not need the rest of pnpm to move.
