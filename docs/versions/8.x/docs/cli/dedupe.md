---
id: dedupe
title: "pnpm dedupe"
---

Perform an install removing older dependencies in the lockfile if a newer version can be used.

## Options

### `--check`

Added in: v8.3.0

Check if running dedupe would result in changes without installing packages or editing the lockfile. Exits with a non-zero status code if changes are possible.
