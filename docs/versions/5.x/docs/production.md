---
id: production
title: Production
---

There are two ways to bootstrap your package in a production environment with
pnpm. One of these is to commit the lockfile. Then, in your production
environment, run `pnpm install` - this will build the dependency tree using the
lockfile, meaning the dependency versions will be consistent with how they were
when the lockfile was committed. This is the most effective way (and the one we
recommend) to ensure your dependency tree persists across environments.

The other method is to commit the lockfile AND copy the package store to your
production environment (you can change where with the [store location option]).
Then, you can run `pnpm install --offline` and pnpm will use the packages from
the global store, so it will not make any requests to the registry. This is
recommended **ONLY** for environments where external access to the registry is
unavailable for whatever reason.

[store location option]: npmrc#store-dir
