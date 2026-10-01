---
"pacquet": patch
---

`pnpm -s <script>` runs the script again, with `-s` meaning `--sequential` as it does for `pnpm run -s <script>`. pnpm rejected it with "unexpected argument '-s' found" [#16446](https://github.com/pnpm/pnpm/issues/16446).
