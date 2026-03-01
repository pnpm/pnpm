# Architecture

pax is built on top of the pnpm monorepo. This page describes how pax extends pnpm's architecture and where new functionality lives.

## Relationship to pnpm

pax is a fork, not a wrapper. It shares pnpm's codebase directly and extends it in-tree. The goal is to stay mergeable with upstream pnpm where possible, isolating pax-specific changes into new packages and clearly marked extension points.

## High-level flow (planned)

```
package.config.ts          ←  authored by the user (typed, composable)
       ↓
    pax compile             ←  transpiles to package.json
       ↓
  package.json              ←  standard npm-compatible output
       ↓
    pax install             ←  resolves npm + foreign deps, links into node_modules
       ↓
  node_modules/             ←  content-addressable store (inherited from pnpm)
```

## Key extension points (planned)

- **Config compiler**: Loads `package.config.ts`, evaluates it, and emits `package.json`. Lives in a new top-level package (TBD).
- **Foreign resolvers**: Resolver plugins that translate Cargo, pip, Homebrew, and other registry lookups into npm-shaped package metadata. Extend the existing `resolving/` directory.
- **Translation layer**: Maps foreign package metadata (Cargo.toml fields, PyPI classifiers, etc.) into `package.json` structures. Works alongside the foreign resolvers.
- **Lockfile extensions**: Additions to `pnpm-lock.yaml` to track cross-ecosystem provenance without breaking pnpm compatibility.

## Directory conventions

New pax-specific packages should follow the existing monorepo conventions:

- One concern per package.
- Place packages in the most relevant functional directory (e.g., a new resolver goes in `resolving/`).
- If a package is clearly pax-only and doesn't map to an existing directory, create a new top-level directory with a clear name.
