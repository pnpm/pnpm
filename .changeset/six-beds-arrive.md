---
"@pnpm/config": minor
"pnpm": minor
---

Allow env variables to be specified with default values in `.npmrc`. This is a convention used by Yarn too.
Using `${NAME-fallback}` will return `fallback` if `NAME` isn't set. `${NAME:-fallback}` will return `fallback` if `NAME` isn't set, or is an empty string [#6018](https://github.com/pnpm/pnpm/issues/6018).
