---
"@pnpm/pnpr": minor
---

pnpr now reads every command-line option from an environment variable when the flag is omitted. The variable is named after the flag with a `PNPR_` prefix, so `--public-url` becomes `PNPR_PUBLIC_URL` and `--disable-resolver` becomes `PNPR_DISABLE_RESOLVER`. A flag given on the command line wins over its environment variable. Boolean flags accept `true`, `1`, `yes`, `on`, `false`, `0`, `no`, and `off`.
