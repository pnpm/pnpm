## 12.5.0

pnpm 12.5.0 makes Python a first-class ecosystem, accepts Package URLs in `pnpm add`, names whole platforms in `supportedArchitectures`, and gives tasks machine-wide concurrency limits. It also fixes an install that could reuse one package's downloaded tarball for another.

### Minor Changes

#### Installing packages

- `pnpm add` accepts a [Package URL](https://github.com/package-url/purl-spec) in place of a package name. `pnpm add pkg:npm/express@4.18.2` saves `express` to `package.json`. `pnpm add pkg:cargo/serde@1.0.188` saves `serde` to `Cargo.toml`. `pnpm add pkg:pypi/requests@2.31.0` saves `requests` to `pyproject.toml`. `pkg` is now a reserved specifier prefix, whatever case it is written in, so a named registry can no longer be called `pkg`.

- A `registries` entry can now name the ecosystem it serves.

  ```yaml
  registries:
    https://internal.example/simple/:
      ecosystem: pypi
    https://pypi.org/simple/:
      ecosystem: pypi
    https://index.crates.io/:
      ecosystem: cargo
  ```

  `ecosystem` accepts `npm`, `cargo` and `pypi`. An entry that does not name one serves npm, as every entry did before.

  An ecosystem with several indexes searches them in the order they are declared. The first index that has a package supplies it, so the one declared last answers what none before it had.

  A `registries` entry may not carry credentials. pnpm reads them from `.npmrc`, matched by origin, for a PyPI index as for every other package source.

#### Configuring pnpm

- `supportedArchitectures` now accepts a list of platforms, in place of the `os`, `cpu` and `libc` axes.

  ```yaml
  supportedArchitectures:
    - linux-x64
    - darwin-arm64
    - win32-x64
  ```

  An install prepares for the platforms the list names, and for those only. A platform reads as `<os>-<cpu>`, with a C library on Linux, as in `linux-x64-musl` or `linux-x64-manylinux_2_28`. The Rust target triple of the same machine is accepted too, so `x86_64-unknown-linux-gnu` names the platform `linux-x64` names. A Linux platform that names no C library is the glibc platform. `current` is the platform the install runs on.

  The `os`, `cpu` and `libc` mapping keeps working and keeps its meaning.

- Added concurrency groups for tasks. A task in `pnpm-workspace.yaml` can name a `concurrencyGroup`. The new `concurrencyGroups` setting gives each group a limit. At most that many tasks of the group run at once on the machine, counted across every pnpm process, `pnpm pipeline` included. A task past the limit waits for a running one to finish. A script that calls `pnpm run` for a task of the same group runs under the slot its parent holds.

  ```yaml
  tasks:
    test:rust:
      concurrencyGroup: cargo
  concurrencyGroups:
    cargo: 2
  ```

- `tools` names the programs pnpm downloads, and `mirror` says where each one comes from.

  ```yaml
  tools:
    node:
      mirror: https://mirror.example.com/node/download
      channels:
        nightly: https://nightly.example.com/
    bun:
      mirror: https://mirror.example.com/bun
    python:
      mirror: https://mirror.example.com/python-build-standalone/releases
  ```

  `node`, `bun` and `python` can be named. Any other tool is refused.

  `mirror` is the base a tool's own layout hangs off.

  `channels` sends one release channel elsewhere. A channel neither it nor `node-mirror:<channel>` names is left to `mirror`. Only `node` publishes channels, so naming them for another tool is refused.

  Set it in the global `config.yaml` or in `PNPM_CONFIG_TOOLS`. A `pnpm-workspace.yaml` that names a tool mirror is ignored.

  `pnpm pack-app` downloads the Node.js it embeds through `tools.node`. `node-mirror:<channel>` keeps working and names the same thing as an entry under `channels`.

#### Python interpreters and environments

- `pnpm install` now chooses a Python interpreter for each project instead of installing every project with one interpreter [#14945](https://github.com/pnpm/pnpm/issues/14945). A project is installed with the first interpreter on the machine that its `requires-python` accepts, so a workspace can hold projects that support different Python versions. pnpm reads `.python-version` too, and prefers the version it asks for. Set `python.executable` in `pnpm-workspace.yaml` to name one interpreter for every project.

- `pnpm install` now installs a Python interpreter when no interpreter on the machine fits the project [#14945](https://github.com/pnpm/pnpm/issues/14945). The builds are [python-build-standalone](https://github.com/astral-sh/python-build-standalone)'s, which uv and rye install too. One interpreter is shared by every project on the machine, and a later install uses it without downloading anything. `runtimeOnFail` decides what an install with no interpreter that fits does, the way it does for a Node.js runtime. `error` reports the project instead of installing one. `warn` and `ignore` install with an interpreter the machine has that the project's `requires-python` rejects. `tools.python.mirror` names a mirror.

- Python environments now live in the store. Each project keeps only its `.venv` link, which points at the project's current environment generation under `python-envs` in the store. A repository with many Python projects no longer holds a `.pnpm/python-envs` directory in each of them. The next install relinks a `.venv` that an earlier release published. The old `.pnpm/python-envs` directory is left in place, since a running program may still use it, and can be deleted once none does. With `frozenStore` set, pnpm writes nothing to the store, so environments stay in the project's `.pnpm/python-envs` [#15014](https://github.com/pnpm/pnpm/issues/15014).

- Python environments now use `packageImportMethod` to import wheel files from the store. Use `clone-or-copy` for copy-on-write clones with a copy fallback, or `copy` for independent files. Hardlinked files share writes with the store and other environments.

  Isolated Python build environments keep backend writes private with copy-on-write clones or copies.

#### Python projects and workspaces

- `pnpm install` now installs a Python project's own package, so the project can be imported and the commands in `[project.scripts]` run right after an install [#14945](https://github.com/pnpm/pnpm/issues/14945). The installed package points at the source tree, so an edit to a module takes effect without another install. pnpm installs the package of a project that declares a `[build-system]`. `tool.uv.package` overrides that either way.

- `pnpm install` now installs a Python project in the workspace from its own source. Declare it under `[tool.uv.sources]`, as `shared = { workspace = true }` or `shared = { path = "../shared", editable = true }`. pnpm builds the project with the backend it declares. It installs the build editable, so an edit to the project takes effect without another install.

  Approve the build backend under `allowBuilds` in `pnpm-workspace.yaml` as a Package URL, as `pkg:pypi/hatchling: true`. An install that has not approved a backend does not build the projects that need it. The message names the key to add.

  `pnpm install` now refuses a requirement that names a project in the workspace when nothing declares where it comes from. It used to take that name from the index.

- The members of a uv workspace can now share one Python environment. Set `shared-environment = true` under `[tool.pnpm.python]` in the `pyproject.toml` that declares `[tool.uv.workspace]`. `pnpm install` then resolves every member as one graph into one `pylock.toml` and one `.venv` at the workspace root. Two members that require versions of a distribution no release satisfies at once are refused with an error naming both. Each project still gets an environment of its own by default [#15015](https://github.com/pnpm/pnpm/issues/15015).

- Python projects can now select extras and dependency groups through `[tool.pnpm.python]` in `pyproject.toml` [#14945](https://github.com/pnpm/pnpm/issues/14945). Workspace `python.extras` and `python.groups` defaults now skip names a project does not define.

- `pnpm install` now reads dynamic Python project metadata from the build backend [#14945](https://github.com/pnpm/pnpm/issues/14945). Projects with only a `requirements.txt` file now get a Python environment and lockfile.

#### Python dependencies and lockfiles

- pnpm can now resolve `pylock.toml` for several platforms and Python versions at once. `supportedArchitectures` names the platforms to lock for and `python.versions` the versions. Every platform is paired with every version. One committed lockfile then serves Linux CI and macOS or Windows contributors [#14945](https://github.com/pnpm/pnpm/issues/14945).

  ```yaml
  supportedArchitectures:
    - linux-x64-manylinux_2_28
    - darwin-arm64
    - win32-x64
  python:
    enabled: true
    versions: ['3.12', '3.13']
  ```

  The lockfile pins the wheel each environment takes for a distribution. It marks a package only some environments install. `pnpm install` takes the packages and wheels of the environment its interpreter matches, and refuses an interpreter none of them stand for. pnpm resolves a project that declares environments itself, not through the server `pnprServer` names. Naming neither setting locks for the interpreter running the install.

- `python.overrides` and `python.constraints` pin the versions a Python resolution may pick [#14945](https://github.com/pnpm/pnpm/issues/14945). pnpm reads uv's own overrides and constraints from `pyproject.toml` too.

- `pnpm install` now supports Python dependencies from Git repositories [#14945](https://github.com/pnpm/pnpm/issues/14945). Direct wheel URLs are also supported. Sources can be declared in `[tool.uv.sources]`. Git dependencies require `allowBuilds` approval.

- `pnpm install` can install a Python release that publishes no wheel this interpreter accepts, by building the source distribution the index serves beside it [#14945](https://github.com/pnpm/pnpm/issues/14945). The archive is pinned in `pylock.toml` by name and SHA-256. A later install replays it from the store, offline included. Building a source distribution runs the release's own build backend. Approve it with `pkg:pypi/<distribution>: true` under `allowBuilds`.

  A resolution that finds no version of a distribution now says why. It tells apart a distribution no index publishes, one whose releases publish nothing this interpreter can install, and one whose versions the project's requirements exclude.

### Patch Changes

#### Installing packages

- pnpm no longer reuses one package's downloaded tarball for another package whose resolution pins a different integrity hash to the same URL [#15021](https://github.com/pnpm/pnpm/issues/15021).

- `pnpm install` and `pnpm add` now report an error when `package.json`, `pnpm-lock.yaml`, `pyproject.toml` or another file they snapshot before installing is a named pipe or a device. The command used to wait forever for something to write to it.

- `pnpm install --prod` and `pnpm install --dev` now record every dependency group in `pnpm-lock.yaml`. `node_modules` still holds only the groups the filter selects. They used to write the filter into the lockfile, so a later `pnpm install --frozen-lockfile` rejected it. `pnpm prune --prod`, `pnpm prune --dev`, and `pnpm prune --no-optional` behave the same way [#14912](https://github.com/pnpm/pnpm/issues/14912).

- POSIX bin shims now convert a Windows-form path such as `C:\node_modules\.bin\tsc` correctly. The shim mangled the backslashes in such a path and could not reach the package it runs. Installing again replaces the shims already in `node_modules` [#14867](https://github.com/pnpm/pnpm/issues/14867).

- Two pnpm processes installing one workspace at the same time no longer fail on Windows with "Access is denied" while writing `node_modules/.pnpm-workspace-state-v1.json`. The write now retries the transient lock the other process holds, as pnpm's other file writes do.

- pnpm now reads the manifest from the tarball when a pnpmfile `resolvers` hook returns a resolution without one. Such a package installed alone, with none of its own dependencies and no warning [#15000](https://github.com/pnpm/pnpm/issues/15000).

- `pnpm install` now merges Git conflict markers in `pnpm-lock.yaml`. It parses both sides of the conflict and keeps the versions they locked. A conflict in the config dependencies recorded at the top of the lockfile is merged too [#14880](https://github.com/pnpm/pnpm/issues/14880).

#### Cargo projects

- `pnpm install` can now generate `Cargo.lock` for workspaces with path or Git `[patch]` and `[replace]` overrides. Adding, removing, and updating crates also preserve these overrides [#14950](https://github.com/pnpm/pnpm/issues/14950).

  Cargo lockfile resolution blocks unsupported Git transport helpers declared by transitive dependencies.

- `pnpm install` now vendors recursive Git submodules for Cargo dependencies at their pinned commits. Cargo builds can use these sources offline. Set Git's `protocol.file.allow` to `always` to fetch local file submodules. pnpm fetches cached Git crates again on the first online install [#14951](https://github.com/pnpm/pnpm/issues/14951).

- `pnpm install` now generates `Cargo.lock` for workspaces with Git dependencies, including a dependency that omits a package version. It also downloads the Rust standard library's dependencies when Cargo configuration enables `build-std` [#14944](https://github.com/pnpm/pnpm/issues/14944).

- `pnpm install` now handles weak Cargo features, written `crate?/feature`. Resolution failed when one dependency turned on an optional crate and another asked for a weak feature of it [#14960](https://github.com/pnpm/pnpm/issues/14960). The generated `Cargo.lock` now also includes the dependencies weak features reference, which Cargo rejected with `--locked` for crates such as `uuid` [#14978](https://github.com/pnpm/pnpm/issues/14978).

- `pnpm install` now generates `Cargo.lock` when a crate version it considers depends on a release the registry carries only as yanked. pnpm rules that version out and resolves the rest of the graph. Resolution failed with an error such as `no non-yanked version of napi-build satisfies ^3.0.0-beta` [#14952](https://github.com/pnpm/pnpm/issues/14952).

- `pnpm install` now falls back to an older semver-incompatible version of a crate when the newest one a dependency range allows cannot be resolved. Ranges such as `>=1, <3` span several of them [#14962](https://github.com/pnpm/pnpm/issues/14962).

#### Python projects

- `pnpm install` now honors uv workspace members when discovering Python projects. When no uv workspace declares a project, pnpm skips projects under conventional example, demo, documentation, template, `test`, `tests`, and test fixture directories [#15058](https://github.com/pnpm/pnpm/issues/15058).

- `pnpm install --filter <selector>` now installs only the Python projects the selection asks for. A Python project that shares a directory with an npm workspace project is selected with that project. A Python project in a directory of its own is selected by the distribution it declares, by its path, or through the `[tool.uv.sources]` entries that reach it. Under `--fail-if-no-match`, a selector that names only a Python project is a match. `pnpm add --filter <selector> pypi:<package>` writes the requirement to every selected project [#14945](https://github.com/pnpm/pnpm/issues/14945).

- `pnpm install` now installs wheels whose `RECORD` hashes disagree with their contents. The wheel archive's locked SHA-256 hash remains verified. pnpm writes correct hashes to the installed `RECORD` [#15061](https://github.com/pnpm/pnpm/issues/15061).

- `pnpm install` now installs a Python wheel whose `WHEEL` file lists tags that differ from the ones in its filename. A wheel whose filename tags were changed after the build, such as `mysql-connector-python`, was rejected [#14945](https://github.com/pnpm/pnpm/issues/14945).

- A Python release whose wheel metadata declares a requirement pnpm cannot read no longer fails the install. pnpm now resolves the project against the other releases of that package, and reports the unreadable requirement when none of them works.

- `pnpm install` no longer fails when a Python index lists a file pnpm cannot use, such as a release with no SHA-256 digest or an unreadable wheel filename. That file is left out and the project resolves against the remaining releases.

- `pnpm add pypi:<package>` in a directory that has no `pyproject.toml` now names the missing file and says where to run the command. It used to fail with a bare `No such file or directory (os error 2)` [#14945](https://github.com/pnpm/pnpm/issues/14945).

#### Performance

- `pnpm audit` no longer hangs on dependency graphs with many shared dependencies [#15005](https://github.com/pnpm/pnpm/issues/15005).

- Sped up `pnpm install` in Python workspaces with many projects. Projects now prepare concurrently. Projects with identical registry requirements also share fresh dependency resolutions [#14945](https://github.com/pnpm/pnpm/issues/14945).

- Repeat installs through the Node-API bindings now return "Already up to date" when the project manifests still match `pnpm-lock.yaml`. Before, every such install reinstalled the whole tree. An install also no longer reinstalls when `pnpm-lock.yaml` differs from the installed dependencies only by packages no project depends on or by top-level keys pnpm does not define.

#### Other commands

- `pnpm deploy` now links commands exposed by workspace dependencies into the deployed project's `node_modules/.bin` directory [#14899](https://github.com/pnpm/pnpm/issues/14899).

- `pnpm dlx` and `pnx` now prompt to approve dependency build scripts in interactive terminals [#14943](https://github.com/pnpm/pnpm/issues/14943). Cached packages with pending builds also prompt for approval. Without an interactive terminal, use `--allow-build` to allow the required builds.

- `pnpm add -g` and `pnpm update -g` now ignore incomplete unrelated global package groups when every command from the replaced group is retained. Operations that could remove a global command still require complete ownership information.

- `pnpm pack` now writes tarball entries grouped by file extension and file name, the order npm uses. Packages that ship many same-named files, such as template collections, pack much smaller [#14766](https://github.com/pnpm/pnpm/issues/14766).

- `pnpm outdated --long` fills the Details column with the package homepage again [#14886](https://github.com/pnpm/pnpm/issues/14886).
