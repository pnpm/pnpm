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
  repository?: string | { url?: string, directory?: string }
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
  const spec = parseRegistryPackageSpec(packageSpec)
  const metadata = await fetchFullMetadata(opts, spec.name)
  const data = pickPackageFromMeta(
    pickVersionByVersionRange,
    { preferredVersionSelectors: undefined },
    metadata,
    spec
  )
  if (!data) {
    throw new PnpmError('PACKAGE_NOT_FOUND', `No matching version found for ${spec.name}@${spec.fetchSpec}`)
  }
  return toExtendedPackageInfo(metadata, data)
}

function parseRegistryPackageSpec (packageSpec: string): RegistryPackageSpec {
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

  return {
    name: packageName,
    fetchSpec: subSpec?.fetchSpec ?? 'latest',
    type: (subSpec?.type ?? 'tag') as 'tag' | 'version' | 'range',
  }
}

type PackageMeta = Parameters<typeof pickPackageFromMeta>[2]
type PackageMetaVersion = NonNullable<ReturnType<typeof pickPackageFromMeta>>

async function fetchFullMetadata (opts: Config & ConfigContext, packageName: string): Promise<PackageMeta> {
  const registry = pickRegistryForPackage(opts.registriesByScope, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
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
      authHeaderValue: getAuthHeader(registry, { pkgName: packageName }),
      fullMetadata: true,
    }
  )
  if (fetchResult.notModified) {
    throw new PnpmError('UNEXPECTED_304', `Unexpected 304 response for ${packageName}`)
  }
  return fetchResult.meta
}

function toExtendedPackageInfo (metadata: PackageMeta, data: PackageMetaVersion): ExtendedPackageInfo {
  const versions = metadata.versions ? Object.keys(metadata.versions) : []
  const depsCount = data.dependencies ? Object.keys(data.dependencies).length : 0
  const distTags = metadata['dist-tags']

  return {
    ...data,
    author: typeof data.author === 'object' ? (data.author as { name: string }).name : data.author,
    versions,
    versionsCount: versions.length > 0 ? versions.length : undefined,
    depsCount: depsCount > 0 ? depsCount : undefined,
    distTags,
    'dist-tags': distTags,
    time: metadata.time,
  } as unknown as ExtendedPackageInfo
}
