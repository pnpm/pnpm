---
"@pnpm/config": patch
"pnpm": patch
---

Reverted a fix shipped in v10.29.1, which caused another issue [#10571](https://github.com/pnpm/pnpm/issues/10571).
Reverted fix: Fixed pnpm run -r failing with "No projects matched the filters" when an empty pnpm-workspace.yaml exists [#10497](https://github.com/pnpm/pnpm/issues/10497).

