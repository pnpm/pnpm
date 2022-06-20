---
"pnpm": minor
---

Two new commands added: `pnpm patch` and `pnpm patch-commit`.

`pnpm patch <pkg>` prepares a package for patching. For instance, if you want to patch express v1, run:

```
pnpm patch express@1.0.0
```

pnpm will create a temporary directory with `express@1.0.0` that you can modify with your changes.
Once you are read with your changes, run:

```
pnpm patch-commit <path to temp folder>
```

This will create a patch file and write it to `<project>/patches/express@1.0.0.patch`.
Also, it will reference this new patch file from the `patchedDependencies` field in `package.json`:

```json
{
  "pnpm": {
    "patchedDependencies": {
      "express@1.0.0": "patches/express@1.0.0.patch"
    }
  }
}
```
