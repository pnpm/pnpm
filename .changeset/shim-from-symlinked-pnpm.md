---
"pacquet": patch
---

Global shims such as `node` now work when pnpm runs through a relative symlink, as with a Homebrew install. They were copies of that symlink and did not resolve from the global bin directory [#15691](https://github.com/pnpm/pnpm/issues/15691).
