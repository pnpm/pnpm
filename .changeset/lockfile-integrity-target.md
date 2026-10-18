---
"pacquet": minor
---

Added the `lockfile.integrityTarget` setting. With `integrityTarget: tar`, `pnpm-lock.yaml` pins the hash of every package's uncompressed tar, so the lockfile no longer changes when a registry recompresses its tarballs or serves them as tars. pnpm learns a version's tar hash from the store or by downloading it once. `pnpm audit signatures` ties the tar hash to the signed `.tgz` integrity.

```yaml title="pnpm-workspace.yaml"
lockfile:
  integrityTarget: tar
```
