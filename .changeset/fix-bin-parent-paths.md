---
"@pnpm/bins.resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed missing commands when a package's `bin` paths contain excess parent directory segments. Paths such as `../dist/cli.js` now resolve inside the package [pnpm/pnpm#17586](https://github.com/pnpm/pnpm/issues/17586).
