## 12.0.0-rc.6

### Minor Changes

- Added `pnpm cache path`, which prints the directory pnpm uses for its metadata cache. CI setups can use it to cache that directory — including the lockfile verification log, which lets a job skip re-checking an unchanged lockfile against the configured supply-chain policies.

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

- Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster: the per-version release metadata is cached in the pnpm cache directory after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the noticeable delay on the first `node` invocation in a project pinning an already-downloaded runtime [#13899](https://github.com/pnpm/pnpm/issues/13899).

### Patch Changes

- Corepack can run pnpm 12 again [#13018](https://github.com/pnpm/pnpm/issues/13018). Corepack installs no dependencies and runs no lifecycle scripts, so the native binary that the `pnpm` package normally receives from its platform-specific optional dependency was never there, and `corepack use pnpm@next-12` failed with `MODULE_NOT_FOUND`. The package now ships the `bin/pnpm.mjs` and `bin/pnpx.mjs` entry points Corepack looks for; they fetch the pinned native binary on first use — verified against npm's signature and checksum, honouring `COREPACK_NPM_REGISTRY` and the rest of Corepack's registry environment — and hand over to it. Installing pnpm with a package manager is unaffected and still runs the binary directly, with no Node.js startup in between.

- With a configured `pnprServer`, `pnpm install` skips the server exchanges it does not need, closing the gap where an up-to-date project paid a full resolve round trip that a direct install answered locally [pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904):

  - The repeat-install "Already up to date" fast path now runs with a pnpr server configured.
  - An install whose `pnpm-lock.yaml` still satisfies every manifest skips the server resolve exchange and materializes `node_modules` from the on-disk lockfile.
  - The input-lockfile verification round trip is skipped when the local `lockfile-verified.jsonl` cache already covers the lockfile under the current policy; server-verified and server-resolved lockfiles are now recorded into that cache.
  - Changing the `trustPolicy*`, `minimumReleaseAgeStrict`, or `minimumReleaseAgeExclude` settings now invalidates the repeat-install fast path, matching the TypeScript CLI's workspace-state check.

- The published packages now ship a `THIRD-PARTY-NOTICES.md` file carrying the BSD 2-Clause license of the Yarn code that pnpm's hoisted-layout algorithm and built-in package-compatibility database are derived from.
