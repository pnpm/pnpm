---
id: star
title: pnpm star
---

Mark a package as a favorite on the registry. You must be logged in (see [`pnpm login`](./login.md)). Without credentials, `star` and `unstar` fail with `ERR_PNPM_STAR_UNAUTHORIZED`, and `stars` fails with `ERR_PNPM_STARS_UNAUTHORIZED`. An unknown package fails with `ERR_PNPM_PACKAGE_NOT_FOUND`.

```sh
pnpm star <pkg>
```

## pnpm unstar

Remove a package from your favorites.

```sh
pnpm unstar <pkg>
```

## pnpm stars

List the packages you (or another user) have starred.

```sh
pnpm stars [<user>]
```

When run without a username, pnpm lists the packages starred by the currently authenticated user.

## Options

### --registry &lt;url\>

The registry to use. Defaults to the configured default registry. `star`, `unstar`, and `stars` all accept it.
