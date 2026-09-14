## 1100.2.1

### Patch Changes

- Windows filesystem operations now retry permission errors for up to one second. Permanent permission errors previously delayed failure by a minute. Sharing and lock violations retain their one-minute retry budget [pnpm/pnpm#14682](https://github.com/pnpm/pnpm/issues/14682).
