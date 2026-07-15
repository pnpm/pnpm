## 11.13.1

### Patch Changes

- Fixed `pnpm pack` applying workspace-root ignore rules when a workspace package has its own `.npmignore` file.

- Keep the interactive `minimumReleaseAge` approval prompt visible during `pnpm install`. The progress reporter now pauses its redraws while a prompt is waiting for input instead of overwriting it, so the install no longer hangs on a question the user cannot see [#13019](https://github.com/pnpm/pnpm/issues/13019).

- Fixed `pnpm self-update` failing to link native platform binaries stored in sibling global virtual store slots.
