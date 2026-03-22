---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Fixed a path traversal vulnerability in tarball extraction on Windows. The path normalization was only checking for `./` but not `.\`. Since backslashes are directory separators on Windows, malicious packages could use paths like `foo\..\..\.npmrc` to write files outside the package directory.
