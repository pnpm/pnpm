// The scripts that `pnpm run` and `pnpm install` execute
// are likely to also execute other `pnpm run`.
// We don't want this potentially expensive check to repeat.
// The solution is to use an env key to disable the check.
export const DISABLE_DEPS_CHECK_ENV = {
  npm_config_verify_deps_before_run: 'false' as const,
}
