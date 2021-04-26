---
"@pnpm/package-requester": major
---

Breaking changes to the API of `packageRequester()`.

`resolve` and `fetchers` should be passed in through `options`, not as arguments.

`cafs` is not returned anymore. It should be passed in through `options` as well.
