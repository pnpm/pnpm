---
"pacquet": patch
---

With `autoDedupe` enabled, `pnpm install --lockfile-only` no longer resolves the dependency graph again when nothing changed since an earlier `--lockfile-only` install deduplicated the lockfile. Such an install keeps the lockfile even if versions were published since it was written, or if only a setting such as `resolutionMode` changed. Run `pnpm dedupe` to apply such a change [#16458](https://github.com/pnpm/pnpm/issues/16458).
