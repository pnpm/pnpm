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

const SCRIPTS_TO_SKIP = [
  'preinstall',
  'install',
  'postinstall',
  'preuninstall',
  'uninstall',
  'postuninstall',
]

export const shouldRunCheck = (env: Env, scriptName: string): boolean =>
  !env[SKIP_ENV_KEY] && !SCRIPTS_TO_SKIP.includes(scriptName) && !SCRIPTS_TO_SKIP.includes(env.npm_lifecycle_event!)
