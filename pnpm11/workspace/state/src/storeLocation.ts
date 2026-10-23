import path from 'node:path'

import { STORE_VERSION } from '@pnpm/constants'

import type { WorkspaceStateSettings } from './types.js'

type StoreLocationSettings = Pick<WorkspaceStateSettings, 'storeDir' | 'virtualStoreDir'>

export interface StoreLocationBaseDirs {
  /** The directory of the workspace state, which `virtualStoreDir` resolves against. */
  stateDir: string
  /** The directory `storeDir` resolves against: the workspace, else the project. */
  storeBaseDir: string
}

/**
 * `storeDir` and `virtualStoreDir` as the paths the installer resolves them
 * to, so a relative and an absolute spelling of one directory compare equal.
 * A setting that is not configured stays unset.
 */
export function resolveStoreLocationSettings (settings: StoreLocationSettings, dirs: StoreLocationBaseDirs): StoreLocationSettings {
  return {
    storeDir: settings.storeDir ? resolveStoreDir(settings.storeDir, dirs.storeBaseDir) : undefined,
    virtualStoreDir: settings.virtualStoreDir ? path.resolve(dirs.stateDir, settings.virtualStoreDir) : undefined,
  }
}

function resolveStoreDir (storeDir: string, baseDir: string): string {
  const resolved = path.resolve(baseDir, storeDir)
  return path.basename(resolved) === STORE_VERSION ? resolved : path.join(resolved, STORE_VERSION)
}
