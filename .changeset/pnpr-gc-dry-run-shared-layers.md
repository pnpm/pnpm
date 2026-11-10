---
"@pnpm/pnpr": patch
---

`pnpr gc --dry-run` no longer lists OCI image blobs as removable while another repository in the same organization still keeps them [zkochan/pnpm-tasks#50](https://github.com/zkochan/pnpm-tasks/issues/50).
