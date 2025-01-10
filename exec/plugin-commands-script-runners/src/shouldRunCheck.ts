// We don't want to run check again in a nested script.
export interface Env extends NodeJS.ProcessEnv {
  npm_lifecycle_event?: string
}

export const shouldRunCheck = (env: Env): boolean => env.npm_lifecycle_event == null
