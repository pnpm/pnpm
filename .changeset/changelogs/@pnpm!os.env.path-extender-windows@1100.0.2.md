## 1100.0.2

### Patch Changes

- `pnpm run` and `pnpm setup` no longer clear the visible terminal on Windows when a project path contains non-ASCII characters. Command shims and `pnpm setup` now switch the console code page with `mode.com con cp select=` instead of `chcp.com`; changing the code page with `chcp.com` clears the screen.
