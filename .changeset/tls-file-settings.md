---
"@pnpm/config.commands": patch
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

TLS file paths in `.npmrc` and `auth.ini` resolve relative to the file that declares them. Project `.npmrc` files skip TLS file paths containing environment variables.

`pnpm config get` refuses to print protected auth settings. `pnpm config list` hides TLS private keys and inline proxy credentials. `pnpm config set` writes an unscoped `tokenHelper` to the selected location's auth file.
