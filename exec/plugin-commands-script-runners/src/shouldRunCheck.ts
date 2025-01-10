// We don't want to run check again in a nested script.
export interface Env extends NodeJS.ProcessEnv {
  npm_lifecycle_event?: string
}

const EVENTS_TO_SKIP: Array<string | undefined> = [
  'preinstall',
  'install',
  'postinstall',
  'preuninstall',
  'uninstall',
  'postuninstall',
]

export const shouldRunCheck = (env: Env): boolean => !EVENTS_TO_SKIP.includes(env.npm_lifecycle_event)
