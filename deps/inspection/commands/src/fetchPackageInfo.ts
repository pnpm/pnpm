import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import {
  fetchMetadataFromFromRegistry,
  pickPackageFromMeta,
  pickVersionByVersionRange,
  type RegistryPackageSpec,
} from '@pnpm/resolving.npm-resolver'
import type { PackageInRegistry } from '@pnpm/resolving.registry.types'

export type ExtendedPackageInfo = PackageInRegistry & {
  author?: string
  repository?: string
  versions: string[]
  versionsCount?: number
  depsCount?: number
  distTags: Record<string, string>
  'dist-tags': Record<string, string>
  time?: Record<string, string>
}

export async function fetchPackageInfo (
  opts: Config & ConfigContext,
  packageSpec: string
): Promise<ExtendedPackageInfo> {
  let parsed: ReturnType<typeof npa>
  try {
    parsed = npa(packageSpec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name: "${packageSpec}"`)
  }

  if (!parsed.registry) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name: "${packageSpec}". This command only supports registry packages.`)
  }

  const subSpec = parsed.type === 'alias' ? parsed.subSpec : parsed
  const packageName = subSpec?.name
  if (!packageName) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name: "${packageSpec}"`)
  }

  const specType = (subSpec?.type ?? 'tag') as 'tag' | 'version' | 'range'
  const spec: RegistryPackageSpec = {
    name: packageName,
    fetchSpec: subSpec?.fetchSpec ?? 'latest',
    type: specType,
  }
  const registry = pickRegistryForPackage(opts.registries, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, opts.registries?.default)
  const fetchResult = await fetchMetadataFromFromRegistry(
    {
      fetch: fetchFromRegistry,
      retry: {
        factor: opts.fetchRetryFactor,
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
      },
      timeout: opts.fetchTimeout ?? 60000,
      fetchWarnTimeoutMs: 10000,
    },
    packageName,
    {
      registry,
      authHeaderValue: getAuthHeader(registry),
      fullMetadata: true,
    }
  )
  if (fetchResult.notModified) {
    throw new PnpmError('UNEXPECTED_304', `Unexpected 304 response for ${packageName}`)
  }
  const { meta: metadata } = fetchResult
  const data = pickPackageFromMeta(
    pickVersionByVersionRange,
    { preferredVersionSelectors: undefined },
    spec,
    metadata
  )
  if (!data) {
    throw new PnpmError('PACKAGE_NOT_FOUND', `No matching version found for ${packageName}@${spec.fetchSpec}`)
  }

  const versions = metadata.versions ? Object.keys(metadata.versions) : []
  const depsCount = data.dependencies ? Object.keys(data.dependencies).length : 0
  const distTags = metadata['dist-tags']

  return {
    ...data,
    author: typeof data.author === 'object' ? (data.author as { name: string }).name : data.author,
    repository: typeof data.repository === 'object' ? (data.repository as { url: string }).url : data.repository,
    versions,
    versionsCount: versions.length > 0 ? versions.length : undefined,
    depsCount: depsCount > 0 ? depsCount : undefined,
    distTags,
    'dist-tags': distTags,
    time: metadata.time,
  } as unknown as ExtendedPackageInfo
}
