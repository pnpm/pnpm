# Python ecosystem integration

This implements the Python vertical integration for [pnpm/pnpm#14566](https://github.com/pnpm/pnpm/issues/14566).
Python participates in the real pnpm v12 CLI alongside npm and Cargo. It is
opt-in and does not introduce a stable ecosystem adapter API.

## Using it

```yaml
# pnpm-workspace.yaml
python:
  enabled: true
  # Defaults shown below; all are optional.
  executable: python3
  indexUrl: https://pypi.org/simple/
  extras: []
  groups: [dev]
```

On Windows the default executable is `python`. The interpreter needs `venv`
and either `packaging` or pip's bundled copy of `packaging`. The host helper
uses these for interpreter tags, environment markers and wheel layout.
It performs no dependency resolution or network requests.

```sh
pnpm install
pnpm add 'pypi:requests[socks]@>=2,<3'
pnpm add pypi:pytest --save-dev
pnpm add pypi:orjson --save-exact
pnpm install --offline --frozen-lockfile
pnpm install --prod --frozen-lockfile
pnpm exec python -c 'import requests'
```

Static PEP 621 dependencies come from each discovered `pyproject.toml`.
Tool-only manifests are ignored. Each Python project has its own `pylock.toml`
and `.venv`; environment directories are excluded from discovery.
Python-only operations do not scaffold Node metadata. Mixed adds can contain
npm, `crate:` and `pypi:` selectors together.

`--save-dev` writes `[dependency-groups].dev`; bare requirements save the
resolved lower bound, and `--save-exact` saves an exact Python pin. Explicit
constraints stay intact by default. Unrelated TOML text is preserved.
Selected project extras and dependency groups, including group inclusion,
participate in locking. `--prod` and `--dev` select the installed projection
without changing the complete lockfile. `--lockfile-only` creates no environment.

## Ownership and shared resources

- Python owns PEP 440 versions, PEP 508 requirements, markers and extras.
  PubGrub backtracks over Python versions; Cargo/npm semver is never used.
- The Simple JSON API is negotiated through the existing throttled client.
  Accept headers survive redirects; credentials are reselected for each
  target. Relative artifact URLs use the final response URL.
- Credentials in the configured Python index URL become a Basic authorization
  route and are stripped before caching or locking. Python does not inherit
  npm credentials. Repository-selected indexes cannot choose destinations for
  user-level npm tokens. Routes do not automatically expand to other paths or hosts.
- Python index responses are limited to 64 MiB, including error bodies and
  responses without a content length. The bounded index cache stores raw JSON
  in `python-index-v2`; older index caches require an online refresh.
- Wheel integrity, ZIP extraction, CAS, store-index persistence and download
  reporting use pnpm's shared artifact pipeline. Python has no separate
  downloader, artifact cache or network budget.
- Python verifies wheel identity and RECORD, selects native or pure wheels,
  relocates `.data`, creates entry points, and writes installed RECORD/INSTALLER
  metadata. Environment files are copies, so interpreter writes cannot
  mutate CAS blobs.
- `pnpm run` and `pnpm exec` add environment executables to PATH only when
  Python is enabled. npm-only installs retain the early dispatch path.

## Lockfile contract

The standard [PEP 751 pylock format](https://packaging.python.org/en/latest/specifications/pylock-toml/)
stays separate from `pnpm-lock.yaml`. This implementation writes a single-target,
single-use lockfile with one compatible wheel per distribution. Environment
markers describe the target; `[tool.pnpm]` records resolver inputs for freshness.
Replay verifies artifacts and dependency closure. Cached Simple responses
also permit offline resolution if every selected wheel is in the shared store.

`uv.lock` remains uv's project format. uv already
[installs standard pylock files](https://docs.astral.sh/uv/pip/compile/).
An independent uv sync of the pnpm-generated real-PyPI lockfile installed
the same seven distributions, including native `orjson`.

## Publication and failure semantics

The interpreter creates a complete generation under `.pnpm/python-envs/`.
The coordinator settles all participants before publishing Python environments.
A failed sibling discards staged generations. Existing user-owned `.venv`
directories are never replaced.

Unix publication uses an atomic symlink rename. Windows uses pnpm's directory
link/junction helper. Publication failures restore previous Python links;
generations are retained if rollback itself fails. Successful older generations
remain available to already-running executables.

Mixed adds hold the workspace metadata lock and restore participating manifests
and lockfiles on failure. Cargo and Python use the shared publication barrier
and metadata rollback for ordinary installs as well. npm retains its existing
ordinary-install failure semantics; its metadata and materialized files are not
a global transaction.
Immutable CAS data and metadata caches may survive any failure.
This is in-process rollback, not a crash journal or a globally atomic
multi-directory commit. Automatic generation garbage collection remains open.

## Deliberate limits

This covers the same vertical integration surfaces as the current Cargo
implementation, not all pip or uv functionality. It supports registry wheels
and static project dependencies. Source builds, editable/local/git/URL
requirements, Python installation, dynamic dependency metadata, HTML-only
indexes, pip configuration/keyring discovery, universal multi-target locking
and recursive/filtered add are not implemented. Existing lockfiles must use
pnpm's supported single-target contract; arbitrary third-party pylock imports
are not supported. Unsupported forms fail explicitly.

## Verification

```sh
cargo nextest run --locked -p pnpm-cli -E 'test(python)'
just ready
```

CLI tests use real commands and interpreters. Coverage includes mixed
npm/Cargo/Python installation, imports and console scripts, backtracking,
cycles, extras/markers, group inclusion errors, independent projects,
frozen/offline/prod replay, add freshness and formatting, failed mixed-add
rollback, archive/RECORD corruption, lockfile closure tampering, unmanaged
environments, symlinked generation parents and disabled fast paths.
CI explicitly provisions Python instead of skipping tests when it is absent.

A real PyPI smoke project used `requests[socks]>=2,<3` and `orjson>=3`.
pnpm installed seven distributions; imports and native JSON encoding succeeded.
uv 0.12.10 independently installed the lockfile into another environment.

The Linux integrated benchmark compared release builds against `61dee94773`,
with matching CLI build features, two warmups and 15 measurements per target.
The same unchanged pnpr binary served both targets. All eleven scenarios passed.

| npm-only scenario | Baseline mean (ms) | Integration mean (ms) |
| --- | ---: | ---: |
| Fresh install, cold cache/store | 497.0 | 491.9 |
| Fresh install, hot cache/store | 249.2 | 243.1 |
| Fresh install, cold cache/hot store | 424.9 | 419.5 |
| Frozen restore, cold cache/store | 364.5 | 384.6 |
| Frozen restore, hot cache/store | 112.9 | 113.2 |
| Repeat install, hot cache/store | 5.93 | 5.98 |
| Repeat install, cold cache/hot store | 6.15 | 6.05 |
| Add, hot cache/store | 267.8 | 276.1 |
| Offline resolution, hot cache | 172.7 | 172.9 |
| GVS frozen restore, hot cache/store | 103.8 | 105.2 |
| Frozen restore, cold cache/store/pnpr | 413.2 | 418.5 |

The initial cold-restore and add slowdowns were repeated with reversed target
order, four warmups and 30 measurements. Cold restore was 376.8 ms versus
378.7 ms (standard deviations 7.9 and 10.8 ms); add was 262.3 ms versus
264.8 ms (standard deviations 5.8 and 5.7 ms). The larger initial slowdowns
did not reproduce. The repeat differences, 0.5% and 1.0%, are within observed
variation. These measurements are not a speedup claim.

## Architecture findings

Shared HTTP/auth/CAS contracts work with a real second registry protocol and a
different package layout. Resolution and lockfile semantics remain Python-owned.
No universal package graph or lockfile writer was needed.

Staged publication and authoritative metadata ownership are implemented in
the shared `pnpm-install-coordinator` crate. Cargo and Python produce prepared
projections through the same contract; npm enrolls with its existing in-place
materialization semantics. Both `install` and mixed `add` use this lifecycle.
See [ECOSYSTEM_INSTALL.md](./ECOSYSTEM_INSTALL.md) for ownership, failure semantics
and the extension boundary. Target-dependent projections still need native
interpreter identity; immutable archive identity alone is insufficient.
