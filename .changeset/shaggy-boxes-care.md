---
"pnpm": major
"@pnpm/resolve-dependencies": major
---

A prerelease version is always added as an exact version to `package.json`. If the `next` version of `foo` is `1.0.0-beta.1` then running `pnpm add foo@next` will add this to `package.json`:

```json
{
  "dependencies": {
    "foo": "1.0.0-beta.1"
  }
}
```
