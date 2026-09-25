import { promises as fs } from 'node:fs'

import type { Config } from '@pnpm/config.reader'
import { globalWarn } from '@pnpm/logger'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller'
import { getStorePath, getStorePathInPnpmHome } from '@pnpm/store.path'

import { createNewStoreController, type CreateNewStoreControllerOptions, type FullMetadataPolicyOptions, shouldFetchFullMetadata } from './createNewStoreController.js'

export { createNewStoreController, type CreateNewStoreControllerOptions, type FullMetadataPolicyOptions, shouldFetchFullMetadata }

export type CreateStoreControllerOptions = Omit<CreateNewStoreControllerOptions, 'storeDir'> & Pick<Config,
| 'storeDir'
| 'dir'
| 'pnpmHomeDir'
| 'workspaceDir'
>

export interface StoreControllerHandle {
  ctrl: StoreController
  dir: string
  resolutionVerifiers: ResolutionVerifier[]
}

export async function createStoreControllerCached (
  storeControllerCache: Map<string, Promise<StoreControllerHandle>>,
  opts: CreateStoreControllerOptions
): Promise<StoreControllerHandle> {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  if (!storeControllerCache.has(storeDir)) {
    storeControllerCache.set(storeDir, createStoreController(opts))
  }
  return await storeControllerCache.get(storeDir) as StoreControllerHandle
}

export async function createStoreController (
  opts: CreateStoreControllerOptions
): Promise<StoreControllerHandle> {
  const storeDir = await getStorePath({
    pkgRoot: opts.workspaceDir ?? opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  if (!opts.storeDir) {
    await warnIfHomeStoreIsBypassed(opts.pnpmHomeDir, storeDir)
  }
  return createNewStoreController(Object.assign(opts, {
    storeDir,
  }))
}

const warnedBypassedHomeStores = new Set<string>()

async function warnIfHomeStoreIsBypassed (pnpmHomeDir: string, storeDir: string): Promise<void> {
  const homeStoreDir = getStorePathInPnpmHome(pnpmHomeDir)
  if (storeDir === homeStoreDir || warnedBypassedHomeStores.has(homeStoreDir)) return
  warnedBypassedHomeStores.add(homeStoreDir)
  if (!await isDirectory(homeStoreDir)) {
    warnedBypassedHomeStores.delete(homeStoreDir)
    return
  }
  globalWarn(`The store at ${homeStoreDir} is not used because packages cannot be hard linked from it into this project. Using the store at ${storeDir} instead. Set storeDir to choose the store.`)
}

async function isDirectory (dir: string): Promise<boolean> {
  // Only decides whether to print a warning, so an unreadable path must not fail the command.
  return fs.stat(dir).then((stats) => stats.isDirectory(), () => false)
}
