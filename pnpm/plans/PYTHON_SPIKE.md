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
  executable: null # pnpm chooses one per project
  indexUrl: https://pypi.org/simple/
  extraIndexUrls: []
  overrides: []
  constraints: []
  extras: []
  groups: [dev]
  platforms: []
  pythonVersions: []
  downloads: auto # or never
  downloadUrl: https://github.com/astral-sh/python-build-standalone/releases
```

The interpreter needs `venv` and either `packaging` or pip's bundled copy of
`packaging`. The host helper uses these for interpreter tags, environment
markers and wheel layout. It performs no dependency resolution or network
requests.

### Indexes and dependency rules

`python.extraIndexUrls` adds Simple JSON indexes, searched in listed order before
`python.indexUrl`. The first index containing a distribution supplies all of its
versions. Only a 404 tries the next index; authentication failures and other
errors stop resolution. Versions from different indexes are never combined.
Credentials in each URL are scoped to that URL and removed from lockfiles.
A credential fingerprint separates authenticated index caches, and raw
credentials never appear in cache keys.
Missing index pages are cached for offline resolution too.

`python.overrides` and `python.constraints` accept lists of PEP 508 registry
requirements. An override replaces the version requirement for a matching
name throughout the dependency graph, including its requested extras.
Constraints intersect the permitted versions without adding a dependency.
Markers select where a rule applies.
URL requirements are not accepted in these lists. Version rules preserve the
source of dependencies declared as Git repositories or direct wheel URLs.
For example:

```yaml
python:
  enabled: true
  extraIndexUrls: [https://download.example.org/simple/]
  overrides: ['urllib3>=2']
  constraints: ['urllib3<3']
```

pnpm also reads `[tool.uv]` `override-dependencies` and
`constraint-dependencies`. Within a declared uv workspace these lists come
from its root manifest, and otherwise from the project's manifest. They are
combined with the pnpm workspace settings. These rules apply to project
dependencies, not isolated build-backend dependencies. Changes to the index
list or dependency rules invalidate the lockfile. These projects resolve
locally when a pnpr server is configured.

### Choosing an interpreter

`executable` names one interpreter for every project in the workspace. Without
it, each project gets the first interpreter this machine has that its
`requires-python` accepts, so projects that support different Python versions
can live in one workspace.

pnpm tries `python3` and `python` first, then the version a `.python-version`
file asks for by name, then every `python3.<minor>` on the `PATH`, newest
first, and on Windows the versions the launcher reports. A project that
constrains nothing therefore starts one interpreter, as it did before pnpm
chose them. Each interpreter is a candidate by its own path, so one minor
version installed in two directories is two candidates.

pnpm starts the interpreter it located rather than the name it looked for,
and looks only outside the workspace being installed: an install that runs
from a script has the workspace's own `bin` directories on its `PATH`, where
a dependency can leave an executable named like an interpreter.

The nearest `.python-version` file, searched from the project up to the
workspace root, asks for a version: `3.13` asks for every 3.13.x. The file
belongs to other tools as well, so a line naming a distribution rather than a
version is reported and ignored.

### Installing an interpreter

A project no interpreter on the machine fits gets one installed, from the
[python-build-standalone] builds uv, rye, hatch and mise install too. The
release's `SHA256SUMS` is both the list of what pnpm can install and the
digest each download is checked against, and it is cached for a day.

An interpreter is installed under `<store>/python/`, so every project and
repository on the machine shares one, and it is found there afterward like
any other interpreter: a later install uses it without reading the release,
offline included. `downloads: never` keeps pnpm from installing any, and an
offline install installs none; both report the project instead. `downloadUrl`
names a mirror of the releases.

A `.python-version` pin is met by installing the version it names. Where the
release publishes no such version for this machine, or the project's own
`requires-python` refuses it, pnpm says which of the two it is and goes on:
with an interpreter the machine has that `requires-python` accepts, or, where
it has none, by installing the newest version the release does publish that
`requires-python` accepts.

[python-build-standalone]: https://github.com/astral-sh/python-build-standalone

```sh
pnpm install
pnpm add 'pypi:requests[socks]@>=2,<3'
pnpm add pypi:pytest --save-dev
pnpm add pypi:orjson --save-exact
pnpm install --offline --frozen-lockfile
pnpm install --prod --frozen-lockfile
pnpm exec python -c 'import requests'
```

PEP 621 dependencies come from each discovered `pyproject.toml`. Dynamic
version, dependency, extra, and interpreter metadata comes from the backend's
`prepare_metadata_for_build_wheel` hook, falling back to building a wheel when
the hook is absent. Metadata preparation uses the same isolated environments
and `allowBuilds` approval checks as project builds. It also runs during
lockfile-only and frozen installs, since dependency metadata is needed to
validate the lockfile. Unapproved requirements are an error when metadata
cannot be read without executing the backend. Final wheel dependencies, Python
compatibility, and declared extras must match the metadata used for resolution.
Only metadata produced by the preparation hook is passed to a later wheel build.

Directories with `requirements.txt` and no `[project]` table participate too.
A `[project]` table takes precedence over a neighboring requirements file.
Requirements files accept PEP 508 requirements, comments, line continuations,
and local `-r`/`--requirement` includes relative to the containing file. Cyclic
includes and unsupported pip directives, including constraints, editable
requirements, index options, and hashes, produce an error with file context.
Includes must stay inside the discovered workspace, or the project directory
when no workspace is configured. Only regular files are read. Reads and include
depth are bounded, and repeated includes are parsed once.
Direct URL requirements retain the installer's existing restrictions.
Requirements files do not cause the directory's own package to be built or
create a `pyproject.toml`.

Tool-only manifests without requirements are ignored. Each Python project has
its own `pylock.toml` and `.venv`; environment directories are excluded from
discovery.

A requirement on another project in this repository is declared under
`[tool.uv.sources]`, the table every Python workspace in the wild already
writes, and a member inherits the table its workspace root declares:

```toml
[project]
dependencies = ["mylib"]

[tool.uv.sources]
mylib = { workspace = true }
docs = { path = "./docs", editable = true }
```

Such a project is built with its PEP 517 backend in an environment holding
only what that backend asked for, including the requirements the backend names
only once it can see the project, and installed editable unless the source says
otherwise. It is recorded in `pylock.toml` as a PEP 751 directory package whose
path is relative to the lockfile. `[tool.uv.workspace]` says which projects a
workspace contains; without one, every project pnpm discovered is one pnpm may
link. A requirement naming a project in the workspace that no source declares is
refused rather than resolved from the index.

A backend runs code the index served, so pnpm builds a project only where
`allowBuilds` names its build requirements, or `dangerouslyAllowAllBuilds` is
set. Approval covers the names a build asks pnpm to fetch: what
`build-system.requires` declares, PEP 517's setuptools defaults where a project
names no backend, and what the backend adds through
`get_requires_for_build_wheel`/`get_requires_for_build_editable` once it can see
the project. All three are read the same way: a requirement a marker excludes is
not one this target builds with, and one naming a project in the same workspace
is refused rather than taken from the index. What those packages themselves depend on follows from approving
them, the way a dependency's own closure does for a build script. An in-tree
backend reached through `backend-path` is the repository's own code, which pnpm
runs as it runs a workspace project's scripts. What a build environment holds is
resolved against the index each time rather than pinned in `pylock.toml`, as pip
and uv also resolve build requirements, so a lockfile does not fix which release
of a backend a later install runs. A build environment is built once per install
and shared by every project declaring the same requirements. An `allowBuilds` key names a Python distribution
as a [Package URL](https://github.com/package-url/purl-spec), as
`pkg:pypi/hatchling`, because approving is a statement about one piece of code
and npm and PyPI both publish `esbuild`, `ruff` and `black`. A Python version is
not a semver range, so a key naming a version approves nothing here and says so:
a build is approved before its environment is resolved, and there is no version
to check one against yet. An
install that has not approved a build requirement does not build the projects
needing it, which `strictDepBuilds` makes an error rather than a warning.

The environment also holds the project's own package, built the same way, so the
project can be imported, its `[project.scripts]` run, and its
`[project.entry-points]` found from it. A project is packaged when it declares a
`[build-system]`, and `tool.uv.package` overrides that either way. A project the
repository only depends on is built whichever it declares, as uv builds one,
falling back to the backend PEP 517 names when a project declares none.
Python-only operations do not scaffold Node metadata. Mixed adds can contain
npm, `crate:` and `pypi:` selectors together.

`--save-dev` writes `[dependency-groups].dev`; bare requirements save the
resolved lower bound, and `--save-exact` saves an exact Python pin. Explicit
constraints stay intact by default. Unrelated TOML text is preserved.
Selected project extras and dependency groups, including group inclusion,
participate in locking. `--prod` and `--dev` select the installed projection
without changing the complete lockfile. `--lockfile-only` creates no environment.

Workspace `python.extras` and `python.groups` are defaults. Each project selects
only the names it defines, so a workspace can request `groups: [dev, test]` when
some members have only `dev`. Projects override either list independently in
their `pyproject.toml`:

```toml
[tool.pnpm.python]
extras = ["cli"]
groups = ["test"]
```

An empty list disables that workspace default for the project. Explicit project
selections must exist in that project. A selected group's `include-group`
references must exist and cannot form a cycle. Extras use Python's normalized
names. A dependency on a local project's extra still selects that extra through
the dependency requirement, independently of the project's install settings.

## Git and URL requirements

`pnpm install` accepts PEP 508 direct wheel and git requirements, including
requirements a dependency's wheel declares. `[tool.uv.sources]` can also select
these sources. A member inherits its workspace root's sources table.

```toml
[project]
name = "app"
version = "1.0"
dependencies = [
  "fork>=1",
  "theme @ https://example.org/theme-1.0-py3-none-any.whl",
]

[tool.uv.sources]
fork = { git = "https://example.org/fork.git", rev = "e87a64d" }
```

Git sources accept `rev`, `tag` or `branch`, and an optional `subdirectory`.
The direct form is `fork @ git+https://example.org/fork.git@e87a64d` with an
optional `#subdirectory=python` fragment. HTTPS, SSH and local file repositories
are supported. pnpm records the full commit in `packages.vcs` and replays that
commit even if a branch or tag moves. The package is built through the same
isolated PEP 517 flow as workspace packages. Repositories without a
`pyproject.toml` use PEP 517's default setuptools backend.
Git checkouts retain history and tags for backends that derive package versions
from the repository. Cached repositories store Git bundles. Offline installs
restore committed files in a fresh checkout without importing cached hooks or Git configuration.

Approve the git dependency itself, as well as its build requirements, under
`allowBuilds`, using `pkg:pypi/fork: true`. A build may execute code shipped in
the repository, including an in-tree backend, so approving its backend alone
does not approve the dependency. Resolving git metadata may build a wheel,
including during `--lockfile-only`.

Direct HTTP(S) wheel URLs record a SHA-256 digest. Public HTTP URLs require a
`#sha256=...` pin. HTTPS and loopback HTTP URLs can be used without a pin. A `#sha256=...` fragment
provides the expected digest; without one pnpm computes it from the downloaded
wheel. Both the artifact identity and the digest are checked before installation.
Git commits and verified wheels cached by an earlier install can be replayed
with `--offline --frozen-lockfile`. Installed distributions record their source
in PEP 610 `direct_url.json`.

URL source archives remain unsupported. Source declarations selected by a
marker, extra or group are still refused. Direct sources resolve locally;
a pnpr server does not fetch or build git repositories.

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

## The environments a lockfile covers

`platforms` and `pythonVersions` name the environments `pylock.toml` is
resolved for. Every platform is paired with every Python version, and a list
left empty is the platform or the Python version of the interpreter running
the install. Declaring neither locks for that interpreter alone.

A platform names an architecture and a system, either as a Rust target
triple or as an architecture and the libc baseline its wheels are built
against. `linux`, `macos` and `windows` are short names for the most common
three:

```yaml
python:
  enabled: true
  platforms:
    - x86_64-manylinux_2_28
    - aarch64-apple-darwin
    - x86_64-pc-windows-msvc
  pythonVersions: ['3.12', '3.13']
```

A declared environment is a CPython interpreter built against the standard
ABI. A triple ending in
`-unknown-linux-gnu`, and `linux`, are resolved against glibc 2.17;
`-unknown-linux-musl` against musl 1.2; an Apple platform against macOS 14.0.

A Python version written as a minor version is resolved as that minor's first
release, which is the oldest interpreter the environment covers. A release
that requires a later patch release is therefore not locked for it, and a
project that wants one names the version in full. A requirement whose marker
reads a patch release is refused outright rather than locked for part of the
series.

An install refuses an interpreter none of the declared environments stand
for: which packages that interpreter installs, and which wheels it takes, are
exactly what a declared environment answers. Declared environments are
resolved locally rather than through a pnpr server, which answers for one
interpreter.

## Lockfile contract

The standard [PEP 751 pylock format](https://packaging.python.org/en/latest/specifications/pylock-toml/)
stays separate from `pnpm-lock.yaml`. A lockfile pins the wheel every
environment it covers takes for each distribution, and carries the marker
saying which of them install it; a package every environment installs carries
none. `[tool.pnpm]` records the resolver inputs, including the declared
environments, or the marker environment and wheel tags of the interpreter a
project that declares none was resolved for. Each `environments` marker names
the interpreter version, the marker variables the solved graph reads, and
whatever a declared environment fixes, which is what a PEP 751 installer
checks before installing it. For the running interpreter the version is the
minor one unless a locked package's `Requires-Python` tells patch releases
apart.

A declared environment answers for a platform and a Python version and
nothing else. A requirement whose marker it leaves undecided, a kernel
release among them, is refused rather than locked under a claim the
resolution did not make.

A lockfile is replayed on any target that still installs it: the
requirements, index and `requires-python` must be the ones it was resolved
for, every package the interpreter's markers select must pin a wheel carrying
tags it accepts, and the locked graph must be exactly what those markers
select. The recorded environment is not compared, so a kernel update or a
different tag order does not invalidate the lockfile, while a requirement gated on `platform_release` still does when the
markers now select another graph. A lockfile whose graph no longer matches is
resolved again with a warning, and so is one resolved for another target that
pins a wheel the install cannot fetch; under `--frozen-lockfile` both are
errors. Replay
verifies artifacts and dependency closure. Cached Simple responses also permit
offline resolution if every selected wheel is in the shared store.

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
implementation, not all pip or uv functionality. It supports registry wheels,
static and dynamic project dependencies, requirements files, Git and wheel URL
requirements, and the projects in this repository a project depends on.
HTML-only indexes, pip configuration/keyring discovery and recursive/filtered
add are not implemented, nor is building a project with a backend the workspace itself
declares, which is refused rather than taken from the index. A distribution an
index serves only as a source archive is not built. The environments a lockfile covers are resolved one at a
time rather than forked out of one universal solve, so two environments no
marker tells apart cannot need different versions of a distribution.
A release's requirements are read once for the version, from the first wheel
of it that pnpm downloads, so a release whose wheels carry different
`Requires-Dist` or `Requires-Python` is locked from whichever of them that
was. Existing lockfiles must use pnpm's supported contract; arbitrary
third-party pylock imports are not supported. Unsupported forms fail
explicitly.

Dynamic project metadata is prepared for each project's selected interpreter.
If dynamic `requires-python` selects another interpreter, pnpm prepares the
metadata again with that interpreter. Backends
that generate different dependencies for other interpreters or platforms are
not queried for each lockfile environment. Path sources outside the discovered
project inventory still need static metadata. `pnpm add pypi:` writes static
manifest dependencies and does not edit backend-owned dynamic metadata.

Interpreters already on the machine are found by name, not by reading the
registries and version-manager directories a Python installation can hide in.
The ones pnpm installs are the current build of each version line the latest
python-build-standalone release offers, so a project pinning a patch release
that release has moved past is installed with another patch and a warning,
rather than with the exact one. pnpm builds no interpreter itself, so a
platform that release does not publish for has none to install.

## Verification

```sh
cargo nextest run --locked -p pnpm-cli -E 'test(python)'
just ready
```

CLI tests use real commands and interpreters. Coverage includes mixed
npm/Cargo/Python installation, imports and console scripts, backtracking,
cycles, extras/markers, group inclusion errors, independent projects,
frozen/offline/prod replay, locking for several platforms at once, add
freshness and formatting, failed mixed-add rollback, archive/RECORD corruption, lockfile closure tampering, unmanaged
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
