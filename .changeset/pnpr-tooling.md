---
"@pnpm/pnpr": patch
---

Added maintenance subcommands to `pnpr`. `config check` inspects configuration diagnostics, `verify` audits hosted documents and storage blobs, `export` dumps packages and authentication records, `migrate-storage` migrates packages from local storage to S3, and `import-htpasswd` imports Apache and Verdaccio user accounts with automatic bcrypt upgrade.
