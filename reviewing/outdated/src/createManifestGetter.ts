import {
  type ClientOptions,
  createResolver,
  type ResolveFunction,
} from '@pnpm/client'
import { createMatcher } from '@pnpm/matcher'
import { type DependencyManifest } from '@pnpm/types'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  rawConfig: object
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, bareSpecifier: string) => Promise<DependencyManifest | null> {
  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    filterMetadata: Boolean(opts.minimumReleaseAge),
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge),
  })
  return getManifest.bind(null, resolve, opts)
}

export async function getManifest (
  resolve: ResolveFunction,
  opts: GetManifestOpts,
  packageName: string,
  bareSpecifier: string
): Promise<DependencyManifest | null> {
  let publishedBy: Date | undefined
  if (opts.minimumReleaseAge) {
    const isExcluded = opts.minimumReleaseAgeExclude && createMatcher(opts.minimumReleaseAgeExclude)(packageName)
    if (!isExcluded) {
      publishedBy = new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000)
    }
  }

  try {
    const resolution = await resolve({ alias: packageName, bareSpecifier }, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy,
    })
    return resolution?.manifest ?? null
  } catch (err) {
    if ((err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION' && publishedBy) {
      // No versions found that meet the minimumReleaseAge requirement
      return null
    }
    throw err
  }
}
