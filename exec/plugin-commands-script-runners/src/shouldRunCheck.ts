// The scripts that `pnpm run` and `pnpm install` execute
// are likely to also execute other `pnpm run`.
// We don't want this potentially expensive check to repeat.
// The solution is to use an env key to disable the check.
export const SKIP_ENV_KEY = 'pnpm_run_skip_deps_check'
export const DISABLE_DEPS_CHECK_ENV = {
  [SKIP_ENV_KEY]: 'true',
} as const satisfies Env

export interface Env extends NodeJS.ProcessEnv {
  [SKIP_ENV_KEY]?: string
  npm_lifecycle_event?: string
}

export const shouldRunCheck = (env: Env): boolean => !env[SKIP_ENV_KEY] && !env.npm_lifecycle_event
