## 1102.3.2

### Patch Changes

- A registry prefix or an override version reference named like a built-in object property, such as `constructor` or `$toString`, is now handled correctly. The prefix was rejected as declared by two registries, and the override resolved to a function.

- pnpm now fails with `ERR_PNPM_INVALID_ALLOW_BUILDS` when `allowBuilds` is not an object or one of its values is not `true`, `false`, or a string. Such values used to be ignored silently.
