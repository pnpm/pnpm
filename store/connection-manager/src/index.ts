import type { Config } from '@pnpm/config.reader'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller'
import { getStorePath } from '@pnpm/store.path'

import { createNewStoreController, type CreateNewStoreControllerOptions } from './createNewStoreController.js'

export { createNewStoreController }

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
  return createNewStoreController(Object.assign(opts, {
    storeDir,
  }))
}
