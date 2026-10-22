---
id: patch-remove
title: "pnpm patch-remove <pkg...>"
---

Remove existing patch files and settings in `patchedDependencies`.

```sh
pnpm patch-remove foo@1.0.0 bar@1.0.1
```

The command deletes the patch files and removes their keys from
[`patchedDependencies`](./patch.md#patcheddependencies). If the patches
directory is empty afterwards, it is deleted too.

Without arguments, pnpm lists the recorded patches and asks which ones to
remove. If no patches are recorded, the command fails with
`ERR_PNPM_NO_PATCHES_TO_REMOVE`. A key that is not in `patchedDependencies`
fails with `ERR_PNPM_PATCH_NOT_FOUND`.
