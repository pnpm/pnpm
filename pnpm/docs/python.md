---
id: python
title: Python dependencies
---

Added in: v12.4.0 (pnpm v12 only)

:::warning

Multi-ecosystem support is experimental. The settings and the layout it writes may change.

:::

pnpm can install a project's Python dependencies alongside its npm packages. One `pnpm install` resolves both graphs, uses pnpm's connection budget, and stores every verified wheel in pnpm's content-addressable store, so a wheel fetched for one project is reused by the next.

Turn it on in `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
python:
  enabled: true
```

pnpm reads the project's `pyproject.toml`, writes a [`pylock.toml`](https://packaging.python.org/en/latest/specifications/pylock-toml/), and builds an environment for it.

## Adding a package

Prefix the requirement with `pypi:`:

```sh
pnpm add pypi:httpx
pnpm add pypi:httpx@0.28.1
pnpm add pypi:'httpx>=0.28'
pnpm add -D pypi:pytest
```

A bare `name@version` is written as an exact pin (`httpx==0.28.1`). A requirement that already carries a comparison operator is kept as written, and the full [PEP 508](https://peps.python.org/pep-0508/) grammar is accepted, environment markers and extras included.

## The environment

Each discovered Python project gets a `.venv` in its own directory, unless its workspace [shares one](#sharing-one-environment). It is a symlink into `python-envs` in the pnpm store, and pnpm swaps the link atomically when the environment changes, so a failed install leaves the previous environment in place.

Since v12.5.0, environments live in the store and each project keeps only its `.venv` link. The next install migrates links created by earlier releases. Old project-local `.pnpm/python-envs` directories are left in place and can be deleted once no running program uses them. With [`frozenStore`](./settings/store.md#frozenstore), environments remain under the project's `.pnpm/python-envs` instead.

pnpm refuses to touch a `.venv` it did not create, so an existing hand-made virtual environment is never replaced.

[`pnpm run`](./cli/run.md) and [`pnpm exec`](./cli/exec.md) put the environment's script directory (`.venv/bin`, or `.venv/Scripts` on Windows) at the front of `PATH`, so a script can call `pytest` or `ruff` without activating anything.

Python requirement, marker, and lockfile semantics are kept separate from npm's and Cargo's. What is shared is the plumbing below them: the HTTP and authentication budget, artifact verification, and the store.

`--lockfile-only`, `--frozen-lockfile`, and `--offline` apply to Python dependencies too.

## Project packages and local sources

Added in: v12.5.0

pnpm installs the project's own package editable when it declares `[build-system]`, making its imports and `[project.scripts]` commands available immediately. Source edits take effect without reinstalling. `[tool.uv].package` overrides whether the project is packaged. Dynamic metadata is read from the declared build backend. A project with only `requirements.txt` can also receive an environment and `pylock.toml`.

Declare workspace and local dependencies in `pyproject.toml`:

```toml title="pyproject.toml"
[project]
name = "app"
version = "0.1.0"
dependencies = ["shared"]

[tool.uv.sources]
shared = { workspace = true }
# Alternatively: shared = { path = "../shared", editable = true }
```

pnpm builds local packages with their declared backend. Editable installations follow source edits. A requirement naming a workspace project without a source declaration is refused instead of fetched from the index.

Build requirements need approval under [`allowBuilds`](./settings/build.md#allowbuilds), using Package URL keys:

```yaml title="pnpm-workspace.yaml"
allowBuilds:
  'pkg:pypi/hatchling': true
```

An unapproved backend is skipped, with a message naming the approval to add. Git sources and source distributions additionally need approval of `pkg:pypi/<distribution>`. pnpm can build a source distribution when no compatible wheel is published; its archive and SHA-256 are pinned in `pylock.toml`, and cached sources can be replayed offline. Direct wheel URLs and Git sources can be declared in `[tool.uv.sources]`; Git and URL sources cannot be installed editable.

## Locking for multiple environments

Added in: v12.5.0

Use [`supportedArchitectures`](./settings/dependency-resolution.md#supportedarchitectures) and `python.versions` to resolve one `pylock.toml` for several platforms and interpreters:

```yaml title="pnpm-workspace.yaml"
supportedArchitectures:
  - linux-x64-manylinux_2_28
  - darwin-arm64
  - win32-x64
python:
  enabled: true
  versions: ['3.12', '3.13']
```

Every platform is paired with every Python version. The lockfile pins wheels and conditional packages for each environment. Installation selects the matching environment and rejects an interpreter outside the declared environments. Without either setting, pnpm locks for the interpreter running the install.

## Filtering projects

Project discovery honors `[tool.uv.workspace]` members. Outside an explicitly declared uv workspace, conventional example, demo, documentation, template, test, and fixture directories are skipped.

Since v12.5.0, `pnpm install --filter <selector>` selects Python projects by name, path, or dependencies declared in `[tool.uv.sources]`. A Python project colocated with an npm workspace project is selected with that project. `--fail-if-no-match` accepts Python-only matches, and `pnpm add --filter <selector> pypi:<package>` writes to every selected project. Selecting a member of a shared environment installs the whole environment.

## Selecting extras and dependency groups per project

Added in: v12.5.0

Workspace `python.extras` and `python.groups` are defaults. Each project selects only the names it defines, so members with different extras and groups can install together. For example, `groups: [dev, test]` selects both groups in a project that defines both, and only `dev` in a project that defines just `dev`.

Override either list in the project's `pyproject.toml`:

```toml title="pyproject.toml"
[tool.pnpm.python]
extras = ["cli"]
groups = ["test"]
```

The lists override their workspace defaults independently. Omitting a list keeps its workspace default. An empty list disables that default for the project.

Explicit project selections must exist in that project. Selected groups' `include-group` references must exist and cannot form cycles. Extra names follow Python's normalization rules, so `dev_tools` and `dev-tools` select the same extra. These rules also apply to extras obtained from dynamic build-backend metadata.

Selections participate in locking. `--prod` and `--dev` choose which dependencies to install without changing the complete lockfile. A requirement on another project's extra, such as `library[cli]`, selects that distribution extra independently of `library`'s own installation settings.

## Sharing one environment

Added in: v12.5.0

By default each project resolves its dependencies on its own and gets an environment of its own. The members of a [uv workspace](https://docs.astral.sh/uv/concepts/projects/workspaces/) can share one environment instead. Ask for it in the `pyproject.toml` that declares the workspace:

```toml title="pyproject.toml"
[tool.uv.workspace]
members = ["packages/*"]

[tool.pnpm.python]
shared-environment = true
```

`pnpm install` then resolves every member as one graph into one `pylock.toml` and one `.venv` at the workspace root. Each member still selects its own extras and dependency groups, and the environment holds the union of them. Every member that builds a package is installed into it, and a member that another member requires through `[tool.uv.sources]` is installed as that source asks. One interpreter serves all of them: the first on the machine that every member's `requires-python` accepts, preferring the version the root's `.python-version` asks for. Where the machine has none, pnpm installs one that every member accepts, as it does for a project on its own, with [`runtimeOnFail`](./settings/cli.md#runtimeonfail) controlling what happens when no installed interpreter fits.

Two members that require versions of one distribution no release satisfies at once are refused, with an error naming the distribution and both members. Two members whose own `[project]` tables name the same distribution are refused too, because one environment holds one distribution of a name. Members may require the same dependencies freely; the environment holds each once.

Selecting any member with [`--filter`](./filtering.md) installs the whole shared environment. `pnpm add` in a member writes that member's `pyproject.toml` and the shared `pylock.toml`. [`pnpm run`](./cli/run.md) and [`pnpm exec`](./cli/exec.md) in a member, or in any directory under one, use the `.venv` at the workspace root.

Sharing is decided per workspace, so a repository can share an environment where its projects agree and keep independent ones where they do not.

## Faster resolution through pnpr

With [`pnprServer`](/pnpr/install-acceleration) set, the server resolves the Python graph, so pnpm does not have to download a wheel to find out what it requires. A server that does not answer for Python makes pnpm fall back to resolving locally.

Python graphs configured with multiple indexes, overrides, constraints, or explicit target environments resolve locally. pnpr cannot represent these resolution settings.

## Settings

### python.enabled

* Default: **false**
* Type: **Boolean**

Whether `pnpm install` resolves and installs the project's Python dependencies.

### python.executable

* Default: **automatic selection per project**
* Type: **String**

An explicit interpreter to use for every project. When omitted, pnpm chooses an interpreter that satisfies each project's `requires-python`, preferring `.python-version` when present. If none fits, pnpm downloads a shared [python-build-standalone](https://github.com/astral-sh/python-build-standalone) interpreter. [`runtimeOnFail`](./settings/cli.md#runtimeonfail) controls this: the default and `download` allow downloads, `error` fails, and `warn` or `ignore` use an available interpreter despite the version mismatch. Offline installs cannot download an interpreter. Configure its download source with [`tools.python.mirror`](./settings/cli.md#tools).

### Python indexes

Since v12.5.0, declare indexes through [`registries`](./registries.md#ecosystem), with `ecosystem: pypi`. Without a declaration, pnpm uses `https://pypi.org/simple/`. `python.indexUrl` and `python.extraIndexUrls` are not supported settings in v12.5.0.

Since v12.5.1, assign package names to each index through [`packages`](./registries.md#packages). pnpm selects one authoritative index before requesting a package, so declaration order has no effect.

```yaml title="pnpm-workspace.yaml"
python:
  enabled: true
registries:
  https://packages.example.org/simple/:
    ecosystem: pypi
    packages: ["company-*", "legacy-internal-package"]
  https://download.pytorch.org/whl/cpu/:
    ecosystem: pypi
    packages: ["torch"]
  https://pypi.org/simple/:
    ecosystem: pypi
    packages: ["*"]
```

`packages` accepts exact distribution names, a name prefix followed by a single `*`, and `*` or `**` for the default index. Names are compared after Python name normalization: case is ignored and runs of `.`, `_`, and `-` become `-`, so `Company_Tools` and `company-tools` are one claim. Any other wildcard shape, such as `company-**` or `@scope/*`, is rejected, as is an empty list.

Two patterns that can match the same name are a configuration error, whether they sit in one entry or in two: an entry that claims the default may claim nothing else, and only one index may be the default.

A matched package resolves exclusively from its assigned index, including transitive dependencies and isolated build dependencies. A missing package, incompatible version, authentication error, or other registry failure stops resolution. pnpm does not retry another index. Indexes must support the Simple JSON API; HTML-only indexes are not supported. Missing pages are cached for offline resolution.

Omitting `packages` claims the default too, so a single custom index needs no patterns, and two indexes that both omit it are refused as two defaults. Once any Python index is declared, PyPI is not added implicitly: a package no index claims fails with `ERR_PNPM_UNCLAIMED_PYTHON_PACKAGE` unless one index is the default. Changing the routes invalidates Python lockfiles, and a `--frozen-lockfile` install then fails with `the Python index changed`.

Configure credentials in [`.npmrc`](./npmrc.md), matched by origin, rather than putting them in a `registries` URL. Authenticated index caches use a credential fingerprint; raw credentials never appear in cache keys.

### python.overrides

Added in: v12.5.0

* Default: **[]**
* Type: **String[]**

[PEP 508 registry requirements](https://packaging.python.org/en/latest/specifications/dependency-specifiers/) that replace matching dependency version requirements throughout the Python graph. Environment markers select where an override applies. An active override replaces the original extras with the extras it requests.

An override does not add a dependency that the graph does not already require. URL requirements cannot be used as overrides. Version rules preserve Git or direct wheel sources already declared for a dependency.

### python.constraints

Added in: v12.5.0

* Default: **[]**
* Type: **String[]**

PEP 508 registry requirements that narrow the permitted versions of matching Python dependencies. Constraints do not add dependencies or replace their version requirements. Environment markers select where a constraint applies, and URL requirements cannot be used as constraints.

Overrides and constraints can be used together:

```yaml title="pnpm-workspace.yaml"
python:
  enabled: true
  overrides: ['urllib3>=2']
  constraints: ['urllib3<3']
```

pnpm also reads `[tool.uv]` `override-dependencies` and `constraint-dependencies` from `pyproject.toml`. In a declared uv workspace, these lists come from the workspace root manifest. Otherwise, they come from each project's manifest. They are combined with the pnpm workspace settings.

These rules apply to project dependencies. They do not apply to isolated build-backend dependencies. Changing the index list, overrides, or constraints invalidates `pylock.toml`, and `--frozen-lockfile` rejects those changes.

### Importing wheel files

Python uses [packageImportMethod](./settings/node-modules.md#packageimportmethod) to import unchanged wheel files into project environments. It uses the same methods and `auto` default as npm packages.

Use `clone-or-copy` for copy-on-write clones with a copy fallback, or `copy` for independent files. `clone` requires filesystem support for cloning.

:::warning

Hardlinked wheel files share writes with the store and other hardlinked environments. The `auto` method can select hardlinks. Use `clone-or-copy` or `copy` if installed files may be modified.

:::

Isolated build environments also use `packageImportMethod`, but replace `auto` and `hardlink` with `clone-or-copy` to keep build backend writes private.

Generated metadata, entry-point scripts, and wheel scripts whose shebangs or permissions need changing remain private in every mode.

```yaml title="pnpm-workspace.yaml"
packageImportMethod: clone-or-copy
python:
  enabled: true
```

### python.extras

* Default: **[]**
* Type: **String[]**

The default [extras](https://packaging.python.org/en/latest/specifications/dependency-specifiers/#extras) to install in each project. Names a project does not define are skipped. `[tool.pnpm.python].extras` in the project's `pyproject.toml` overrides this list.

### python.groups

* Default: **['dev']**
* Type: **String[]**

The default [dependency groups](https://peps.python.org/pep-0735/) to install in each project. Names a project does not define are skipped. `[tool.pnpm.python].groups` in the project's `pyproject.toml` overrides this list.

### python.versions

Added in: v12.5.0

* Default: **[]**
* Type: **String[]**

Python minor or full versions to lock for, such as `3.12` or `3.12.7`. An empty list uses the selected interpreter's version. See [locking for multiple environments](#locking-for-multiple-environments).
