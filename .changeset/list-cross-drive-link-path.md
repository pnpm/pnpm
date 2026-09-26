---
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
---

`pnpm list` now shows the correct path of a `link:` dependency that points to a directory on another drive on Windows. The path used to be appended to the project directory, such as `C:\project\D:\lib`, and `pnpm list --long` could not show the package's details [#10362](https://github.com/pnpm/pnpm/issues/10362).
