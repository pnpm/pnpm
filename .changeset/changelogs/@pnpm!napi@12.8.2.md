## 12.8.2

### Patch Changes

- `pnpm install` returns "Already up to date" again in a workspace with injected workspace dependencies and a shared lockfile. Since 12.7.0 every repeat install in such a workspace ran the full install and copied the injected projects again.
