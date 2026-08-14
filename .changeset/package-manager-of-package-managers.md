---
"pacquet": minor
---

pnpm installs the other package managers now, not just itself: npm, Yarn Classic, Yarn Berry, Yarn 6 (`yarnpkg/zpm`), and Bun. Each is resolved and fetched through the trusted package-manager registries, and an npm-published one is verified against npm's signature for its exact version before it is executed.

Three things use it:

- A git-hosted dependency is prepared with the package manager it asks for. Its `packageManager` / `devEngines.packageManager` pin is honored, and a `yarn.lock` written by Yarn Classic no longer gets installed by Yarn Berry. pnpm provides that package manager when the dependency pinned one, or when the host has none — so a repository built with Yarn now installs on a machine that has only pnpm, while a host that already had it keeps using its own.
- `pnpm with yarn@4 install`, `pnpm with npm@11 ci`, `pnpm with bun@1.3.0 install` run a one-off command under any of them. A bare version still means pnpm, so `pnpm with 10.5.0 install` is unchanged.
- `pnpm shim add yarn` links a `yarn` command that runs whatever version the current project pins, and `pnpm shim rm` / `pnpm shim ls` manage those shims. It works for any package, not only package managers. Shims are never created as a side effect of `pnpm setup` or an install — a shim shadows the rest of your `PATH`, so pnpm only writes one when asked.

A JavaScript package manager on a machine without Node.js gets a managed LTS runtime to run on.
