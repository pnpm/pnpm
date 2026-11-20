---
id: registry-revisions
title: Registry Revisions
---

Registry revisions are supported by pnpm v12 and later.

This version of pnpm stops with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` when
it meets one:

* package metadata whose selected version carries `dist.revision` or
  `dist.revisions`;
* a `+rN` version specifier, such as `2.7.4+r1`;
* a lockfile entry with a `revision` field.
