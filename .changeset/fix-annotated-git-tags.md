---
"@pnpm/git-resolver": patch
"pnpm": patch
---

Fix installation of Git dependencies using annotated tags [#10335](https://github.com/pnpm/pnpm/issues/10335).

Previously, pnpm would store the annotated tag object's SHA in the lockfile instead of the actual commit SHA. This caused `ERR_PNPM_GIT_CHECKOUT_FAILED` errors because the checked-out commit hash didn't match the stored tag object hash.
