---
"pacquet": patch
---

The lockfile now records a `catalog:` peer dependency of an injected workspace package as written in its `package.json`. It recorded the catalog's range before, so editing the catalog changed the consumer's lockfile entry [pnpm/tasks#153](https://github.com/pnpm/tasks/issues/153).
