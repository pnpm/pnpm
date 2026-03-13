import type { Config } from '@pnpm/config'
import type { StoreController } from '@pnpm/package-store'
import { getStorePath } from '@pnpm/store-path'
import { createNewStoreController, type CreateNewStoreControllerOptions } from './createNewStoreController.js'

export { createNewStoreController }

export type CreateStoreControllerOptions = Omit<CreateNewStoreControllerOptions, 'storeDir'> & Pick<Config,
| 'storeDir'
| 'dir'
| 'pnpmHomeDir'
| 'workspaceDir'
>

export async function createStoreControllerCached (
  storeControllerCache: Map<string, Promise<{ ctrl: StoreController, dir: string }>>,
  opts: CreateStoreControllerOptions
): Promise<{ ctrl: StoreController, dir: string }> {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  if (!storeControllerCache.has(storeDir)) {
    storeControllerCache.set(storeDir, createStoreController(opts))
  }
  return await storeControllerCache.get(storeDir) as { ctrl: StoreController, dir: string }
}

export async function createStoreController (
  opts: CreateStoreControllerOptions
): Promise<{
    ctrl: StoreController
    dir: string
  }> {
  const storeDir = await getStorePath({
    pkgRoot: opts.workspaceDir ?? opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  return createNewStoreController(Object.assign(opts, {
    storeDir,
  }))
}
