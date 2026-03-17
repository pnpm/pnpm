import type { CustomResolver } from '@pnpm/hooks.types'
import type { InstallOptions } from '@pnpm/installing.deps-installer'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import type { StoreController } from '@pnpm/store.controller-types'
import { createTempStore } from '@pnpm/testing.temp-store'
import type { Registries } from '@pnpm/types'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

export function testDefaults<T> (
  opts?: T & {
    fastUnpack?: boolean
    storeDir?: string
    prefix?: string
    registries?: Registries
    customResolvers?: CustomResolver[]
  },
  resolveOpts?: any, // eslint-disable-line
  fetchOpts?: any, // eslint-disable-line
  storeOpts?: any // eslint-disable-line
): InstallOptions &
  {
    cacheDir: string
    registries: Registries
    storeController: StoreController
    storeDir: string
  } &
  T {
  const { storeController, storeDir, cacheDir } = createTempStore({
    ...opts,
    clientOptions: {
      ...(opts?.registries != null ? { registries: opts.registries } : {}),
      customResolvers: opts?.customResolvers,
      ...resolveOpts,
      ...fetchOpts,
    },
    storeOptions: storeOpts,
  })
  const result = {
    cacheDir,
    registries: {
      default: registry,
    },
    storeController,
    storeDir,
    ...opts,
  } as (
    InstallOptions &
    {
      cacheDir: string
      registries: Registries
      storeController: StoreController
      storeDir: string
    } &
    T
  )
  return result
}
