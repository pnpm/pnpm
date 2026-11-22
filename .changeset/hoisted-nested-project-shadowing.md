---
"pnpm": patch
---

Fixed `nodeLinker: hoisted` installs in a workspace where the directory of one project is inside the directory of another. A dependency of the inner project could resolve to the version in the outer project's `node_modules`. pnpm now keeps the inner project's dependency in its own `node_modules` when the outer project holds another version.
