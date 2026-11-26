---
"pnpm": minor
---

`pnpm view node` now reads the Node.js release index, the same source `pnpm add node` installs from. `pnpm view node versions` lists Node.js releases, and `pnpm view node@22 npm` shows the npm version bundled with the newest Node.js 22 release. Use `node@npm:node` to view the npm package named `node`.
