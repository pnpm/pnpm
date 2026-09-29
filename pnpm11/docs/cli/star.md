---
id: star
title: pnpm star
---

Added in: v11.0.0

Mark a package as a favorite on the registry. You must be logged in (see [`pnpm login`](./login.md)).

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
