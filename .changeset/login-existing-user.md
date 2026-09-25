---
"@pnpm/registry-access.client": patch
"pnpm": patch
"pacquet": patch
---

`pnpm login` now logs back in to an existing user on registries without web login, such as verdaccio. The classic login request sends the username and password as basic auth, as `npm login` does [#12055](https://github.com/pnpm/pnpm/issues/12055).
