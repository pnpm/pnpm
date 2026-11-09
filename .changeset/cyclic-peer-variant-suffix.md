---
"pacquet": patch
---

`pnpm install` now keeps peer dependency variants separate across workspace projects when a cyclic peer dependency's subtree resolves different peers in each project. Before, collapsing the cycle in the peer suffix merged the variants, so a project could receive a peer dependency resolved only in another project [pnpm/pnpm#17883](https://github.com/pnpm/pnpm/issues/17883).
