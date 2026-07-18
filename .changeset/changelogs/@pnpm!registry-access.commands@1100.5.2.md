## 1100.5.2

### Patch Changes

- `pnpm owner ls` now reports authentication and authorization failures (401/403) as dedicated errors that include the registry's response body, matching `pnpm owner add`/`rm`, instead of a generic `Failed to fetch owners` message.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.16
  - @pnpm/config.reader@1101.12.2
