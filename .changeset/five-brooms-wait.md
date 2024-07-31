---
"pnpm": patch
---

`pnpm install` should run `node-gyp rebuild` if the project has a `binding.gyp` file even if the project doesn't have an install script [#8293](https://github.com/pnpm/pnpm/issues/8293).
