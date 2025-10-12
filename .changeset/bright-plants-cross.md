---
"@pnpm/config": patch
"pnpm": patch
---

Fix a bug where pnpm would infinitely recurse when using `verifyDepsBeforeInstall: install` and pre/post install scripts that called other pnpm scripts [#10060](https://github.com/pnpm/pnpm/issues/10060).
