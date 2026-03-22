import type { CookedHooks } from './requireHooks.js'

export type { HookContext } from './Hooks.js'
export { requireHooks } from './requireHooks.js'
export { BadReadPackageHookError } from './requirePnpmfile.js'
export type Hooks = CookedHooks
