---
"pacquet": minor
---

`pnpm add --tilde` is now an alias for `--save-prefix=~`, matching Yarn and reducing migration friction, especially for Yarn Classic users. `-T` was not added, because it would conflict with the proposed `--(install-)types` option [pnpm/pnpm#3868](https://github.com/pnpm/pnpm/issues/3868).
