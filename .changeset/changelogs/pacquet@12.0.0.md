## 12.0.0

### Major Changes

- Git dependencies on known hosts (GitHub, GitLab, Bitbucket) are now treated as identities rather than transport choices. Every representation of the same repository — `github:owner/repo`, `owner/repo`, `git+https://…`, `git+ssh://git@…` — resolves through the host's canonical HTTPS URL, and the lockfile never records an SSH URL for them. Repositories whose archive endpoint is anonymously reachable resolve to the host's archive (fast tarball download); all others resolve to a `git` clone of the canonical HTTPS URL, which every machine with access to the repository can fetch.

  To reach a private hosted repository over SSH, configure the machine (not the project) with git's own URL rewriting, for example:

  ```sh
  git config --global url."git@github.com:".insteadOf https://github.com/
  ```

  pnpm shells out to `git`, so the rewrite applies to all of pnpm's git operations automatically. URLs of unknown hosts (self-hosted servers) are unaffected and keep their exact URL, including SSH. URLs with embedded credentials are also kept verbatim and never resolve to a host archive.

  This removes the network probing that previously decided between HTTPS and SSH at resolution time, which could record a transport that only worked on the machine that happened to run the resolution (e.g. an SSH URL that broke CI runners without SSH keys).

- A project's `pnpm-workspace.yaml` may no longer carry a setting pnpm does not recognize. Such a setting used to be ignored in silence — a misspelled `minimumReleaseAge` dropped the policy it was meant to set, and nothing said so. Now it is reported, suggesting the closest real setting name when the key looks like a typo, and it fails the command with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` when the project pins a pnpm version the running pnpm satisfies: with the pin honored, the setting cannot be meant for a different pnpm version, so it is a mistake to fix rather than a key to ignore. Everywhere else it is a warning, so a project that has yet to be cleaned up keeps working.

  The `pnpm config` subcommands never fail on such a setting, so a broken file can still be inspected and repaired, and `pnpm config get <key>` prints the value with no warnings at all. Keys the global config file cannot set are likewise split between workspace-only settings (still directed to `pnpm-workspace.yaml`) and settings unknown to this version.

- Dependency cycles are now broken canonically during peer resolution: the members of each cycle are ordered by package id, and the edges that close a cycle are always cut at the same place, no matter where the installation walks into the cycle from. Previously the cut depended on the walk path, so installing the same dependencies could produce different lockfiles depending on importer order or resolution order [#13846](https://github.com/pnpm/pnpm/issues/13846), and a peer-resolution verdict computed for one occurrence of a cyclic package could be wrongly reused at another [#13865](https://github.com/pnpm/pnpm/issues/13865).

  With canonical cycle breaking the lockfile is a pure function of the dependency graph: repeated installs, reordered importers, and reordered dependencies all produce byte-identical lockfiles. Peer dependencies of packages inside a cycle keep nearest-wins resolution along the canonical order, and a dependency edge that closes a cycle references an occurrence of its target resolved at the importer level. On large cycle-heavy workspaces peer resolution is 2–3× faster, uses about 25% less memory, and produces a substantially smaller lockfile (fewer redundant peer variants).

  Existing lockfiles keep working: headless (`--frozen-lockfile`) installs consume them unchanged, and installs that skip resolution leave them untouched. The first install that actually re-resolves (for example after a dependency change) re-keys walk-order-dependent peer variants of cyclic packages once.

- `packageImportMethod: auto` now tries hardlinks before cloning on Linux. A reflink materializes a new inode and copies extent bookkeeping inside the filesystem's metadata trees, where a hardlink is one directory entry — on btrfs this roughly halves the time an install spends materializing `node_modules` from a warm store. ext4 installs are unchanged (cloning was never supported there, so `auto` already hardlinked), and macOS keeps clone-first, where APFS `clonefile` is the platform's cheap primitive. Cloning remains the fallback when the store refuses hardlinks, and remains available explicitly via `packageImportMethod: clone`.

- Under `engineStrict`, an install fails when an incompatible package is reached through a regular `dependencies` edge of an installable package, even when that whole subtree hangs off an `optionalDependencies` entry. pnpm v11 installs the package and emits an install-check warning instead. Packages reachable only through optional edges, or through a package that was itself skipped, are still skipped in both versions [#13286](https://github.com/pnpm/pnpm/issues/13286).

### Minor Changes

- Globally installed bins can now follow the project you run them in. The new `globalShims` setting is a record of package names to policies that selects which globally installed packages get project-aware shims; it defaults to `{ node: true, deno: true, bun: true }` and merges key-wise, so `globalShims: { bun: false }` switches one default off and `globalShims: { typescript: true }` adds another package. With the default, a project that pins Node.js through `devEngines.runtime` or `engines.runtime` gets the pinned stable release — authenticated against the Node.js release-team signatures — downloaded on first use and run whenever you type `node` inside the project, with no shell hooks. Candidates that are not signature-verified (Deno, Bun, Node.js prereleases, and ordinary package bins you enable) ask "Do you trust this project?" once per candidate and remember the answer machine-locally; the record values name the policy per package: `"auto"` (or its shorthand `true`) defers to artifact authentication, `"always"` switches without ever asking (useful in CI), and `"prompt"` always asks, even for authenticated candidates. Set `globalShims: false` to disable the feature, or `PNPM_SHIM_BYPASS=1` to bypass it for one invocation. On Windows, programs can keep spawning the global `node.exe` directly, without a shell.

- pnpm installs the other package managers now, not just itself: npm, Yarn Classic, Yarn Berry, Yarn 6 (`yarnpkg/zpm`), and Bun. Each is resolved and fetched through the trusted package-manager registries, and an npm-published one is verified against npm's signature for its exact version before it is executed.

  Three things use it:

  - A git-hosted dependency is prepared with the package manager it asks for. Its `packageManager` / `devEngines.packageManager` pin is honored, and a `yarn.lock` written by Yarn Classic no longer gets installed by Yarn Berry. pnpm provides that package manager when the dependency pinned a version, or when the host cannot satisfy what the dependency needs — so a repository built with Yarn now installs on a machine that has only pnpm, while a host that already has a suitable one keeps using its own.
  - `pnpm dlx` (`pnx`) runs one of them for a single command: `pnx yarn@4 install`, `pnx npm@11 ci`, `pnx bun@1.3.0 install`. Naming a package manager, or a runtime (`node`, `deno`, `bun`), there now provisions the real thing instead of installing the npm package that shares its name — unless the specifier locates a package rather than asking for a released version (`pnx yarn@npm:yarn@1.22.22`, `pnx yarn@yarnpkg/berry`), which installs what it names — `pnx yarn@4` was previously a missing version, since Yarn 4 is published as `@yarnpkg/cli-dist`, and `pnx node@22` now runs that Node.js release rather than a wrapper that downloads one. `--package` naming a package manager picks which of its commands to run, so `pnx --package npm@11 npx create-something` runs that npm's `npx`.
  - `pnpm shim add yarn` links a `yarn` command that runs whatever version the current project pins, and `pnpm shim rm` / `pnpm shim ls` manage those shims. It works for any package, not only package managers. Shims are never created as a side effect of `pnpm setup` or an install — a shim shadows the rest of your `PATH`, so pnpm only writes one when asked.

  Installing a package manager globally (`pnpm add -g yarn`) now makes it follow a project's pin too, the way a globally installed Node.js already follows `devEngines.runtime`: the pinned version runs where a project pins one, and the globally installed copy is the fallback everywhere else. An explicit `globalShims` entry, including `false`, is left as you set it.

  `pnpm add` follows the same rule about what a name means. `pnpm add -g yarn@4` installs Yarn Berry — it used to fail, because npm's `yarn` package stops at Classic — and `pnpm add -g node@22` / `pnpm add -g deno@2` install that Node.js or Deno release rather than a wrapper package that downloads one. In a project, naming a package manager records which one the project uses instead of installing it as a dependency, and naming a runtime records it under `engines.runtime` as `node@runtime:22` already did.

  The declaration goes where the package manager reads it. Yarn is started from a project pin by corepack, which reads only `packageManager` and only accepts an exact version there, so `pnpm add yarn@4` resolves the line and writes `"packageManager": "yarn@4.18.0"` — the same thing `corepack use yarn@4` writes, down to the `+sha512.…` integrity for the Yarn Classic line that corepack pins its tarball with. Every other package manager is recorded in `devEngines.packageManager`, which holds a range. Only one of the two fields is ever left behind: they declare the same thing, and corepack refuses to run a project whose declarations disagree.

  A JavaScript package manager on a machine without Node.js gets a managed LTS runtime to run on.

  What changes for a project coming from v11: `pnpm add yarn` records the project's package manager instead of installing the npm package that shares the name (that package is still reachable as `pnpm add yarn@npm:yarn@1.22.22`), `pnpm add -g yarn` installs the current Yarn line rather than Classic, `pnpm add -g node` / `pnpm add -g deno` and `pnx node` / `pnx deno` install a Node.js or Deno release rather than a wrapper package, and a globally installed package manager defers to a project's pin where there is one.

- Added an opt-in proof of concept that lets installs reuse a dependency's build output across machines, by publishing and restoring signed, organization-scoped artifacts through pnpr instead of running the lifecycle scripts locally.

  Configure it with the new `remoteSideEffectsCache` setting. A workspace names the eligible `organization` and `packages`; everything describing the act of signing — `publish`, `keyId`, `builderId`, `trustedKeys`, `privateKey` and the provenance fields — is refused in `pnpm-workspace.yaml` and read from the global config file or the environment instead.

- Added the `audit.ignorePrune` setting. When set to `true`, `pnpm audit --fix` removes ignored GHSA entries that no longer appear in the audit report.

- `pnpm init` now pins the latest pnpm version, instead of the version of pnpm that ran the command. A project scaffolded by an outdated pnpm therefore no longer inherits that staleness through its own `devEngines.packageManager` / `packageManager` pin [#7490](https://github.com/pnpm/pnpm/issues/7490).

  The version is read from the `latest` tag on the package-manager registries. When that lookup cannot answer — no network, an unreachable or slow registry, `offline`, or a `latest` that the `minimumReleaseAge` / `trustPolicy` settings reject — `pnpm init` pins the running version as before, and never fails or hangs on the lookup. A `latest` that is older than the running pnpm is never pinned either.

- Allowed `pnpm update --patches` to refresh registry revisions through a configured pnpr server while retaining locked package versions.

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

- Added support for registry replacement tarballs using standard integrity values, explicit revision fields, registry routing from the `registries` setting, non-redirecting integrity-addressed URLs, canonical safe-integer revision numbers, and pnpr proxying for immutable upstream revision artifacts.

- Running `pnpm setup`, `pnpm self-update`, or a command that modifies the global installation (such as `pnpm add --global`) through `sudo` now fails with `ERR_PNPM_SUDO_NOT_SUPPORTED` instead of silently operating on the root user's home directory. pnpm keeps global packages and configuration in the invoking user's home directory, so these commands never need root permissions. Read-only global commands (such as `pnpm bin --global`) still work under sudo.

- `pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Inside a workspace, the selected packages are approved in dependency order, and a package whose workspace dependency could not be approved is skipped instead of being published against a dependency that never reached the registry.

### Patch Changes

- Deprecated the pnpmfile `filterLog` hook in pnpm v12. The Rust CLI ignores it and emits a warning.

- The built-in compatibility database no longer adds dependencies that were detected by static analysis of published packages. Those entries named packages that are only imported for their types, so installing them was at best unnecessary and at worst broke the dependent: `@typescript-eslint/types` gained a `typescript` dependency resolved to the newest release, which put TypeScript 7 under older `@typescript-eslint` versions and made ESLint fail with "Cannot read properties of undefined (reading 'Intrinsic')". The database keeps its `@yarnpkg/extensions` entries and pnpm's own curated ones.

- When no directory above the project accepts a hard link — inside an AI agent sandbox that only grants write access to the project, or a container with just the project mounted writable — the default store is now created at `<project>/node_modules/.pnpm-store` instead of in the pnpm home directory. In those environments the home store is either read-only or on another volume, which forces every package to be copied instead of hard linked [#13525](https://github.com/pnpm/pnpm/issues/13525).
