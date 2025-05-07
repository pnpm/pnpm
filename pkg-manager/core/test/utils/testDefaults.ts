import { createTempStore } from '@pnpm/testing.temp-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { type StoreController } from '@pnpm/store-controller-types'
import { type Registries } from '@pnpm/types'
import { type InstallOptions } from '@pnpm/core'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

export function testDefaults<T> (
  opts?: T & {
    fastUnpack?: boolean
    storeDir?: string
    prefix?: string
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
      ...resolveOpts,
      ...fetchOpts,
    },
    storeOptions: storeOpts,
  })
  const result = {
    cacheDir,
    neverBuiltDependencies: [] as string[],
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
