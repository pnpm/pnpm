---
"@pnpm/package-requester": patch
"@pnpm/store.cafs": patch
"pnpm": patch
---

When several containers use the same store simultaneously, there's a chance that multiple containers may create a temporary file at the same time. In such scenarios, pnpm could fail to rename the temporary file in one of the containers. This issue has been addressed: pnpm will no longer fail if the temporary file is absent but the destination file exists.
