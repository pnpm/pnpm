# pax

A universal package manager built as a fork of [pnpm](https://github.com/pnpm/pnpm).

pax brings cross-ecosystem interoperability to JavaScript package management. Use dependencies from npm, Cargo, pip, Homebrew, and more — pax transparently resolves and translates everything into npm-compatible packages so your tools, bundlers, and runtimes keep working as expected.

## Goals

- **Cross-ecosystem dependencies.** Declare dependencies from Cargo, pip, Homebrew, and other package managers alongside your npm packages. pax resolves them, translates their metadata, and wires them into your project.
- **TypeScript-first package configuration.** Replace `package.json` with `package.config.ts` — a fully typed configuration file that gives you autocompletion, type-checking, and the full power of TypeScript at the config level.
- **Transparent npm compatibility.** Everything pax produces is consumable by standard npm tooling. When full npm compatibility isn't feasible, pax targets full pnpm compatibility as a baseline.
- **All of pnpm's strengths.** Content-addressable storage, fast installs, strict dependency isolation, workspace support, and deterministic lockfiles carry over from the upstream project.

## Roadmap

### Phase 1 — Foundation
- [ ] `package.config.ts` support: TypeScript package configuration with strong typing, autocompletion, and validation
- [ ] Transparent compilation of `package.config.ts` down to a standard `package.json`
- [ ] Shared TypeScript config utilities (common types, helpers, and presets importable by `package.config.ts` files)

### Phase 2 — Cross-ecosystem resolution
- [ ] Resolver plugins for non-npm registries (Cargo crates, PyPI packages, Homebrew formulae)
- [ ] Translation layer that maps foreign package metadata into npm-compatible `package.json` structures
- [ ] Lockfile extensions to track cross-ecosystem provenance

### Phase 3 — Bidirectional translation
- [ ] Emit `Cargo.toml`, `requirements.txt`, and other ecosystem manifests from a pax project
- [ ] Allow non-JS projects to consume pax-managed packages through generated native manifests

## How it works (planned)

```
package.config.ts          ←  you author this (typed, composable)
       ↓
    pax compile             ←  pax transpiles to package.json
       ↓
  package.json              ←  standard npm-compatible output
       ↓
    pax install             ←  resolves npm + foreign deps, links into node_modules
       ↓
  node_modules/             ←  content-addressable store (same as pnpm)
```

## TypeScript package configuration (planned)

```ts
// package.config.ts
import { definePackage } from 'pax/config'

export default definePackage({
  name: 'my-app',
  version: '1.0.0',
  dependencies: {
    // npm packages — business as usual
    react: '^18.0.0',
    next: '^14.0.0',
  },
  // foreign dependencies resolved and translated by pax
  foreign: {
    cargo: {
      'wasm-bindgen': '^0.2',
    },
    pip: {
      numpy: '>=1.24',
    },
  },
})
```

## Background

pax is a fork of pnpm, which uses a content-addressable filesystem to store all files from all module directories on a disk. When using npm, if you have 100 projects using lodash, you will have 100 copies of lodash on disk. With pnpm (and pax), lodash will be stored in a content-addressable storage, so:

1. If you depend on different versions of lodash, only the files that differ are added to the store.
2. All the files are saved in a single place on the disk. When packages are installed, their files are linked from that single place consuming no additional disk space.

pax inherits all of this and extends it with cross-ecosystem interoperability.

## Getting started

pax is in early development. For now, it functions identically to pnpm:

```bash
pnpm install
pnpm run compile
```

See the upstream [pnpm documentation](https://pnpm.io) for current CLI usage.

## License

[MIT](https://github.com/pnpm/pnpm/blob/main/LICENSE)
