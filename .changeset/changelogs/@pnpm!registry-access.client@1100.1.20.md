## 1100.1.20

### Patch Changes

- `pnpm login` now logs back in to an existing user on registries without web login, such as verdaccio. The classic login request sends the username and password as basic auth, as `npm login` does [#12055](https://github.com/pnpm/pnpm/issues/12055).

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/network.web-auth@1101.6.1
