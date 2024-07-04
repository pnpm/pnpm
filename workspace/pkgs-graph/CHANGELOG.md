# @pnpm/workspace.pkgs-graph

## 4.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/npm-resolver@21.0.0
  - @pnpm/types@11.0.0

## 3.0.6

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/npm-resolver@20.0.1

## 3.0.5

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/npm-resolver@20.0.0

## 3.0.4

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/npm-resolver@19.0.4

## 3.0.3

### Patch Changes

- @pnpm/npm-resolver@19.0.3

## 3.0.2

### Patch Changes

- Updated dependencies [43b6bb7]
  - @pnpm/npm-resolver@19.0.2

## 3.0.1

### Patch Changes

- Updated dependencies [cb0f459]
  - @pnpm/npm-resolver@19.0.1

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- ca2be03: When sorting packages in a workspace, take into account workspace dependencies specified as `peerDependencies` [#7813](https://github.com/pnpm/pnpm/issues/7813).
- Updated dependencies [7733f3a]
- Updated dependencies [cdd8365]
- Updated dependencies [43cdd87]
- Updated dependencies [d381a60]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/npm-resolver@19.0.0
  - @pnpm/resolve-workspace-range@6.0.0

## 2.0.14

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/npm-resolver@18.1.0

## 2.0.13

### Patch Changes

- Updated dependencies [33313d2fd]
  - @pnpm/npm-resolver@18.0.2

## 2.0.12

### Patch Changes

- @pnpm/npm-resolver@18.0.1

## 2.0.11

### Patch Changes

- Updated dependencies [cd4fcfff0]
  - @pnpm/npm-resolver@18.0.0

## 2.0.10

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/npm-resolver@17.0.0

## 2.0.9

### Patch Changes

- @pnpm/npm-resolver@16.0.13

## 2.0.8

### Patch Changes

- Updated dependencies [01bc58e2c]
- Updated dependencies [ff55119a8]
  - @pnpm/npm-resolver@16.0.12

## 2.0.7

### Patch Changes

- @pnpm/npm-resolver@16.0.11

## 2.0.6

### Patch Changes

- @pnpm/npm-resolver@16.0.10

## 2.0.5

### Patch Changes

- 41c2b65cf: Respect workspace alias syntax in pkg graph [#6922](https://github.com/pnpm/pnpm/issues/6922)
- Updated dependencies [41c2b65cf]
  - @pnpm/npm-resolver@16.0.9

## 2.0.4

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/resolve-workspace-range@5.0.1

## 2.0.3

### Patch Changes

- 9fd0e375e: Speed up createPkgGraph when directory specifiers are present

## 2.0.2

### Patch Changes

- 35d98c7a8: Speed up createPkgGraph by using a table for manifest name lookup

## 2.0.1

### Patch Changes

- 572068180: Optimize createPkgGraph by calling Object.values only once

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/resolve-workspace-range@5.0.0

## 1.0.0

### Major Changes

- 313702d76: Project renamed from `pkgs-graph` to `@pnpm/workspace.pkgs-graph`.
