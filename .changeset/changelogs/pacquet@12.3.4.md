## 12.3.4

### Patch Changes

- Sped up dependency resolution in large workspaces [#14352](https://github.com/pnpm/pnpm/issues/14352).

- pnpm 12 now accepts the boolean settings as command-line flags on every command that takes them in pnpm 11, for example `pnpm install --unsafe-perm`, `pnpm add foo --offline`, and `pnpm install --dangerously-allow-all-builds`. pnpm 12 rejected them with `unexpected argument`, which failed every install on Vercel, whose build runs `pnpm install --unsafe-perm` [#14346](https://github.com/pnpm/pnpm/issues/14346).

  `pnpm remove` now accepts `--unsafe-perm`, the same flag `pnpm install`, `pnpm add`, and `pnpm update` take.
