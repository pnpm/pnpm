---
"@pnpm/config": patch
---

fix a bug where pnpm would infinitely recurse when using `verifyDepsBeforeInstall: install` and pre/post install scripts that called other pnpm scripts (#10060)
