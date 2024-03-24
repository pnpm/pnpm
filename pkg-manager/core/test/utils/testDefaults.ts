import * as path from 'node:path'

import { createClient } from '@pnpm/client'
import { getStorePath } from '@pnpm/store-path'
import { createPackageStore } from '@pnpm/package-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import type { Registries, StoreController, InstallOptions } from '@pnpm/types'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

const retryOpts = {
  retries: 4,
  retryFactor: 10,
  retryMaxtimeout: 60_000,
  retryMintimeout: 10_000,
}

export async function testDefaults<T>(
  opts?: T & {
    fastUnpack?: boolean | undefined
    storeDir?: string | undefined
    prefix?: string | undefined
  },
  resolveOpts?: any | undefined, // eslint-disable-line
  fetchOpts?: any | undefined, // eslint-disable-line
  storeOpts?: any | undefined // eslint-disable-line
): Promise<
  InstallOptions & {
    cacheDir: string
    registries: Registries
    storeController: StoreController
    storeDir: string
  } & T
  > {
  const authConfig = { registry }

  const cacheDir = path.resolve('cache')

  const { resolve, fetchers } = createClient({
    authConfig,
    rawConfig: {},
    retry: retryOpts,
    cacheDir,
    ...resolveOpts,
    ...fetchOpts,
  })

  let storeDir = opts?.storeDir ?? path.resolve('.store')

  storeDir = await getStorePath({
    pkgRoot: opts?.prefix ?? process.cwd(),
    storePath: storeDir,
    pnpmHomeDir: '',
  })

  const storeController = await createPackageStore(resolve, fetchers, {
    ignoreFile:
      opts?.fastUnpack === false
        ? undefined
        : (filename) => filename !== 'package.json',
    storeDir,
    verifyStoreIntegrity: true,
    ...storeOpts,
  })

  const result = {
    cacheDir,
    registries: {
      default: registry,
    },
    storeController,
    storeDir,
    ...opts,
  } as InstallOptions & {
    cacheDir: string
    registries: Registries
    storeController: StoreController
    storeDir: string
  } & T

  return result
}
