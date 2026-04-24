---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix `pnpm update --no-cache` (and similar `--no-X` forms) creating registry metadata under `./false/metadata-full-v1.3/*` instead of the default cache directory. `--no-cache` is not a real flag; nopt prefix-matches `cache` to the `String`-typed `cache-dir` and coerces the negation into the literal string `"false"`, which then gets joined as a relative path by the resolver. The config reader now drops that coercion artifact so the default cache directory is used [#11353](https://github.com/pnpm/pnpm/issues/11353).
