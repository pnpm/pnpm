## 1103.0.1

### Patch Changes

- Under `nodeLinker: hoisted`, peer-resolution variants of an injected directory dependency (a `file:` snapshot) are materialized as separate copies again instead of collapsing onto the first-seen variant. Each copy keeps its own peer-resolved dependency set, so a project pinning one peer version no longer resolves another project's variant — Bit root components with conflicting peers across injected copies rely on this.

- Updated dependencies:
  - @pnpm/building.during-install@1102.0.18
  - @pnpm/deps.graph-builder@1101.0.1
  - @pnpm/installing.linking.real-hoist@1100.1.15
  - @pnpm/lockfile.fs@1100.2.4
  - @pnpm/lockfile.to-pnp@1101.0.1
