## 1100.0.20

### Patch Changes

- Fixed missing commands when a package's `bin` paths contain excess parent directory segments. Paths such as `../dist/cli.js` now resolve inside the package [pnpm/pnpm#17586](https://github.com/pnpm/pnpm/issues/17586).
