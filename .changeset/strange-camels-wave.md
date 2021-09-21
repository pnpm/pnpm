---
"@pnpm/plugin-commands-installation": minor
---

Globally installed packages should always use the active version of Node.js. So if webpack is installed while Node.js 16 is active, webpack will be executed using Node.js 16 even if the active Node.js version is switched using `pnpm env`.
