import requireHooks from './requireHooks'
import type { CookedHooks } from './requireHooks'
import requirePnpmfile, { BadReadPackageHookError } from './requirePnpmfile'

export { requireHooks, requirePnpmfile, BadReadPackageHookError }
export type Hooks = CookedHooks