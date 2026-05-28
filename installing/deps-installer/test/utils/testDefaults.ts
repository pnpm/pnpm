import type { CustomResolver } from '@pnpm/hooks.types'
import type { InstallOptions } from '@pnpm/installing.deps-installer'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller-types'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
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
    minimumReleaseAge?: number
    minimumReleaseAgeStrict?: boolean
    minimumReleaseAgeExclude?: string[]
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
    resolutionVerifiers: ResolutionVerifier[]
  } &
  T {
  // Forward minimumReleaseAge policy into the Client so it builds the
  // matching ResolutionVerifier; tests that set these options exercise the
  // same code path the CLI command would.
  const policyClientOptions = {
    ...(opts?.minimumReleaseAge != null ? { minimumReleaseAge: opts.minimumReleaseAge } : {}),
    ...(opts?.minimumReleaseAgeStrict != null ? { minimumReleaseAgeStrict: opts.minimumReleaseAgeStrict } : {}),
    ...(opts?.minimumReleaseAgeExclude != null ? { minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude } : {}),
  }
  const { storeController, storeDir, cacheDir, resolutionVerifiers } = createTempStore({
    ...opts,
    clientOptions: {
      ...(opts?.registries != null ? { registries: opts.registries } : {}),
      customResolvers: opts?.customResolvers,
      ...policyClientOptions,
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
    resolutionVerifiers,
    ...opts,
  } as (
    InstallOptions &
    {
      cacheDir: string
      registries: Registries
      storeController: StoreController
      storeDir: string
      resolutionVerifiers: ResolutionVerifier[]
    } &
    T
  )
  return result
}
