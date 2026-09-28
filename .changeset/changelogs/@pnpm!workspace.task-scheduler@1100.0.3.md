## 1100.0.3

### Patch Changes

- `pnpm -r run /regexp/` now honors the `tasks` `dependsOn` declared for each script the selector matches, like running the script by name does. Matched scripts that depend on each other run in order. Each matched script runs once [#15596](https://github.com/pnpm/pnpm/issues/15596).

- Updated dependencies:
  - @pnpm/error@1100.2.1
