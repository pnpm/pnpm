---
"pacquet": patch
"@pnpm/pnpr": patch
---

Cleaned up `--help` output by removing markdown that clap printed literally: intra-doc links, an inline link, and a `<...>` placeholder that read as an HTML tag. The affected help text now reads as plain prose — paired flags are named directly (e.g. `--no-offline`) and upstream references appear as plain URLs.
