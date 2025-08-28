---
"@pnpm/config": major
"@pnpm/plugin-commands-config": major
"pnpm": major
---

`pnpm config get` and `pnpm config list` no longer include unknown options and workspace-only options from the `rc` files (global `rc` file, `.npmrc`).

> [!NOTE]
> Before this change, workspace-only options do appear in `pnpm config get` and `pnpm config list` if they are set in `rc` files but **completely useless**. For example: One can set `workspace` like so:
>
> ```ini
> # .npmrc
> packages[]=packages/foo
> packages[]=packages/bar
> ```
>
> And then run `pnpm config get --json packages` to get a JSON array of ["packages/foo", "packages/bar"].
> However, `pnpm install` does not acknowledge such workspace packages.
> Therefore, it was wrong for `pnpm config get` and `pnpm config list` to include them.
