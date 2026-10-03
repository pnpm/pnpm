## 1100.1.21

### Patch Changes

- `pnpm login` no longer forwards credentials in its request body to another origin during redirects.

- A fetch timeout while other downloads from the same host are still running now lowers concurrency for that host to one connection. Retries of that request, and later downloads from that host, use the lower concurrency. Other hosts keep the configured concurrency [pnpm/pnpm#12791](https://github.com/pnpm/pnpm/issues/12791).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.2
