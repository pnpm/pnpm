## 1100.0.4

### Patch Changes

- A `workspace:` dependency now resolves to a workspace project whose version is not valid semver, such as `1` or `1.0`. `workspace:*`, `workspace:^`, and `workspace:~` match it. A range identical to the version also matches it [#4567](https://github.com/pnpm/pnpm/issues/4567).
