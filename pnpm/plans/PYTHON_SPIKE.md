# Python install architecture spike

This experiment continues [pnpm/pnpm#14566](https://github.com/pnpm/pnpm/issues/14566).
It is compiled only in the CLI's unit tests. It adds no command, setting,
specifier protocol, production manifest discovery, or stable adapter API.

Run it from the repository root:

```sh
cargo nextest run -p pnpm-cli -E 'test(python_spike)'
```

## Scope

The fixture adapter reads static `project.dependencies` from discovered
`pyproject.toml` files, resolves exact pins transitively from wheel metadata,
and detects conflicting pins and overlapping installed paths. Names use Python
normalization. Version spellings are compared literally within a deliberately
restricted numeric-release subset. This is not a PEP 440 or PEP 508 resolver.
General requirements must eventually use established Python parsers and a
resolver with Python semantics, not Cargo or npm semver.

The local registry serves the JSON Simple API representation. Each selected
artifact is one `py3-none-any` wheel with a SHA-256 digest. The adapter uses the
existing install HTTP client, URL-routed authorization, ZIP integrity checks,
raw-archive projection, CAS, and store index. It validates wheel identity,
purelib metadata, and SHA-256 RECORD entries before producing a plan.

The plan contains Python-owned file destinations and a `pylock.toml`
using the basic PEP 751 package/wheel fields. Offline replay reads this lock
and the store without requesting index metadata or archives. It revalidates
wheel metadata and the locked dependency closure. The chosen direction is a
standard Python lockfile kept separate from `pnpm-lock.yaml`. The experiment
does not implement general pylock interoperability, target environments, or
manifest freshness checks.

The output is a fixture-owned `site-packages` directory. It is not a virtual
environment installer. Dynamic project metadata, extras, markers, Python
constraints, source builds, native wheels, entry points, `.data` relocation,
folded metadata headers, and general CSV RECORD syntax are unsupported.
Unsupported dependency forms and wheel layouts produce errors; the fixture
reader is not a complete validator of every possible Python project field.

References:

- [Simple repository API](https://packaging.python.org/en/latest/specifications/simple-repository-api/)
- [Wheel format and RECORD verification](https://packaging.python.org/en/latest/specifications/binary-distribution-format/)
- [pylock.toml](https://packaging.python.org/en/latest/specifications/pylock-toml/)

## Why pylock.toml rather than uv.lock

`uv.lock` is uv's project lockfile. Its format can express uv functionality
that pylock cannot, but uv owns that schema and discourages other tools from
depending directly on its lockfiles. `pylock.toml` is the standard interchange
format; uv can export it and install from it through its pip interface.
Keep the standard format for this spike. Revisit a limitation when a concrete
pnpm requirement demonstrates it, rather than adopting uv's schema preemptively.

See uv's [format comparison](https://docs.astral.sh/uv/concepts/projects/layout/#relationship-to-pylocktoml)
and [integration guidance](https://docs.astral.sh/uv/reference/internals/metadata/).

## Contracts exercised

The inventory added in [pnpm/pnpm#14581](https://github.com/pnpm/pnpm/pull/14581)
can return Cargo and Python manifests from one traversal. Interpretation still
belongs to each ecosystem. A generic `pyproject.toml` can contain only tool
configuration, so a production Python reader must decide whether it describes
an installable project. Python environments also need explicit discovery
exclusions before production use.

Verified archive ingestion is reusable without a Python-specific downloader
or store. Wheel RECORD validation remains Python-owned. Cached raw file maps
must be validated again when replayed; a cache hit does not establish Python
metadata correctness. Interpreter-dependent script rewriting and `.data`
relocation would be a separate projection with additional identity inputs.

The authenticated GET helper does not accept a per-request `Accept` header.
The fixture endpoint always serves JSON, but a production Simple API client
must negotiate `application/vnd.pypi.simple.v1+json` while preserving the
existing client, URL auth routing, redirect rules, and network budget. Adding
a second HTTP client to obtain that header would violate the shared contract.
Credential discovery and general Python index configuration remain untested.

## Failure semantics

Resolution and validation produce no workspace writes. Immutable CAS insertions
and raw store-index rows can survive a failed resolution or installation.

The Python plan supplies its full file footprint to `MetadataMutation`, together
with the other ecosystems' metadata paths. The coordinator's settlement mode
waits for all writers before rollback. The regression fixture lets Python
finish its projection, then fails the npm participant after both npm and Cargo
metadata have changed. The original three lockfiles are restored and new
Python files are removed; verified CAS content remains reusable.

This validates in-process file rollback for a fixture-owned destination. It
does not establish crash consistency, safe mutation of an existing virtual
environment, stale-file pruning, interruption recovery, or an atomic switch
of the complete environment. Empty directories can remain after rollback.
The npm and Cargo participants in this test are metadata writers, not complete
invocations of their resolvers. Production integration must also cover those
real install flows and failure during projection publication.

## Performance and next boundary

Every Python module and manifest variant is behind `cfg(test)`. The shipped
CLI's installer selection, discovery basenames, and dependency graph are
unchanged. No third-party dependencies were added.

The integrated benchmark suite was run locally on Linux against baseline
`61dee94773`, with matching release build features, two warmups, and 15 measured
runs per target. All eleven standard scenarios completed. The cold-pnpr scenario
used the pnpr targets with the same unchanged registry binary on both sides.
The largest measured slowdown was 1.5%, within the observed run-to-run spread.
These results show no clear npm-only regression and are not a speedup claim.

| Scenario | Baseline mean (ms) | Spike mean (ms) |
| --- | ---: | ---: |
| Fresh install, cold cache and store | 505.9 | 493.4 |
| Fresh install, hot cache and store | 248.3 | 248.1 |
| Fresh install, cold cache and hot store | 433.1 | 428.1 |
| Frozen restore, cold cache and store | 380.7 | 377.5 |
| Frozen restore, hot cache and store | 109.7 | 111.4 |
| Repeat install, hot cache and store | 6.4 | 6.3 |
| Repeat install, cold cache and hot store | 6.3 | 6.3 |
| Add dependency, hot cache and store | 268.0 | 265.4 |
| Offline resolution, hot cache | 168.3 | 168.7 |
| GVS frozen restore, hot cache and store | 105.7 | 105.2 |
| Frozen restore, cold cache, store and pnpr | 433.3 | 439.3 |

Do not stabilize an adapter trait from this experiment yet. The next Python
slice should exercise negotiated metadata requests and an interpreter-owned
environment projection, with explicit publication and recovery semantics.
General Python resolution needs differential fixtures against an established
Python installer. Those are requirements for a usable Python integration,
not reasons to put Python rules into the shared coordinator.
