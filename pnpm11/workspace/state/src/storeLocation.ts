import path from 'node:path'

import { STORE_VERSION } from '@pnpm/constants'

import type { WorkspaceStateSettings } from './types.js'

type StoreLocationSettings = Pick<WorkspaceStateSettings, 'storeDir' | 'virtualStoreDir'>

/**
 * `storeDir` and `virtualStoreDir` as the paths they name, resolved against
 * the directory of the workspace state, so a relative and an absolute
 * spelling of one directory compare equal. A setting that is not configured
 * stays unset.
 */
export function resolveStoreLocationSettings (settings: StoreLocationSettings, stateDir: string): StoreLocationSettings {
  return {
    storeDir: settings.storeDir ? resolveStoreDir(settings.storeDir, stateDir) : undefined,
    virtualStoreDir: settings.virtualStoreDir ? path.resolve(stateDir, settings.virtualStoreDir) : undefined,
  }
}

function resolveStoreDir (storeDir: string, stateDir: string): string {
  const resolved = path.resolve(stateDir, storeDir)
  return path.basename(resolved) === STORE_VERSION ? resolved : path.join(resolved, STORE_VERSION)
}
