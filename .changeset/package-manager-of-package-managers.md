---
"pacquet": minor
---

pnpm installs the other package managers now, not just itself: npm, Yarn Classic, Yarn Berry, Yarn 6 (`yarnpkg/zpm`), and Bun. Each is resolved and fetched through the trusted package-manager registries, and an npm-published one is verified against npm's signature for its exact version before it is executed.

Three things use it:

- A git-hosted dependency is prepared with the package manager it asks for. Its `packageManager` / `devEngines.packageManager` pin is honored, and a `yarn.lock` written by Yarn Classic no longer gets installed by Yarn Berry. pnpm provides that package manager when the dependency pinned one, or when the host has none — so a repository built with Yarn now installs on a machine that has only pnpm, while a host that already had it keeps using its own.
- `pnpm dlx` (`pnx`) runs one of them for a single command: `pnx yarn@4 install`, `pnx npm@11 ci`, `pnx bun@1.3.0 install`. Naming a package manager or a runtime there now provisions the real thing instead of installing the npm package that shares its name — `pnx yarn@4` was previously a missing version, since Yarn 4 is published as `@yarnpkg/cli-dist`, and `pnx node@22` now runs that Node.js release rather than a wrapper that downloads one.
- `pnpm shim add yarn` links a `yarn` command that runs whatever version the current project pins, and `pnpm shim rm` / `pnpm shim ls` manage those shims. It works for any package, not only package managers. Shims are never created as a side effect of `pnpm setup` or an install — a shim shadows the rest of your `PATH`, so pnpm only writes one when asked.

Installing a package manager globally (`pnpm add -g yarn`) now makes it follow a project's pin too, the way a globally installed Node.js already follows `devEngines.runtime`: the pinned version runs where a project pins one, and the globally installed copy is the fallback everywhere else. An explicit `globalShims` entry, including `false`, is left as you set it.

`pnpm add` follows the same rule about what a name means. `pnpm add -g yarn@4` installs Yarn Berry — it used to fail, because npm's `yarn` package stops at Classic — and `pnpm add -g node@22` installs that Node.js release rather than a wrapper package that downloads one. In a project, naming a package manager records which one the project uses in `devEngines.packageManager` instead of installing it as a dependency, and naming a runtime records it under `engines.runtime` as `node@runtime:22` already did.

A JavaScript package manager on a machine without Node.js gets a managed LTS runtime to run on.
