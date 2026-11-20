## 1100.0.12

### Patch Changes

- pnpm no longer stalls on a patch file whose `diff --git` line contains a long run of spaces. `@pnpm/patching.apply-patch` no longer depends on `@pnpm/patch-package`.

- Patches that add lines to the end of a file without a final newline now install the whole file. pnpm dropped the last added line when the patch also marked it with `\ No newline at end of file` [#18156](https://github.com/pnpm/pnpm/issues/18156).
