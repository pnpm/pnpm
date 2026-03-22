# @pnpm/fetching.binary-fetcher

> A fetcher for binary archives

## Installation

```
pnpm add @pnpm/fetching.binary-fetcher
```

## Testing

### Test Fixtures

The `test/fixtures/` directory contains malicious ZIP files for testing path traversal protection:

| File | Entry Path | Purpose |
|------|------------|---------|
| `path-traversal.zip` | `../../../.npmrc` | Tests `../` escape sequences |
| `absolute-path.zip` | `/etc/passwd` | Tests absolute path entries |
| `backslash-traversal.zip` | `..\..\..\evil.txt` | Tests Windows backslash traversal (Windows-only) |

These fixtures are manually crafted because AdmZip's `addFile()` sanitizes paths automatically.

> **Note:** The backslash test only runs on Windows because `\` is a valid filename character on Unix.

### Regenerating Fixtures

```bash
node --experimental-strip-types scripts/create-fixtures.ts
```

## License

MIT
