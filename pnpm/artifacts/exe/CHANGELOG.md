# @pnpm/exe

## 10.34.8

### Patch Changes

- 07becba: The standalone pnpm executables now embed Node.js 24.22.1. They embedded Node.js 20, which has reached end of life and still trusts the certificate authority intermediates that Node.js distrusted in October 2026.

## 9.5.0

## 7.1.8

### Patch Changes

- 7fb1ac0e4: Fix pre-compiled pnpm binaries crashing when NODE_MODULES is set.

## 6.19.0

### Minor Changes

- b6d74c545: Allow a system's package manager to override pnpm's default settings
