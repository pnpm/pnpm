---
"pacquet": minor
---

`pnpm pack` and `pnpm publish` now warn when the tarball includes a `.env` or `.env.*` file that the `files` field of `package.json` does not list. Templates such as `.env.example` are not reported. List the file in `files` to publish it on purpose, or exclude it in `.npmignore` or `.gitignore` [#7826](https://github.com/pnpm/pnpm/issues/7826).
