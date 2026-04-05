import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
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
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    json: Boolean,
  }
}

export const commandNames = ['view', 'info', 'show', 'v']

export function help (): string {
  return renderHelp({
    description: 'View package information from the registry without using npm CLI.',
    usages: [
      'pnpm view <package-name>',
      'pnpm view <package-name>@<version>',
      'pnpm view <package-name> [<field>[.subfield]...]',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
        ],
      },
    ],
  })
}

export async function handler (
  opts: Config & ConfigContext & {
    json?: boolean
  },
  params: string[]
): Promise<string | void> {
  const packageSpec = params[0]

  if (!packageSpec) {
    throw new PnpmError('MISSING_PACKAGE_NAME', 'Package name is required. Usage: pnpm view <package-name>')
  }

  const fields = params.slice(1)

  let parsed: ReturnType<typeof npa>
  try {
    parsed = npa(packageSpec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name: "${packageSpec}"`)
  }

  if (!parsed.registry) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name: "${packageSpec}". pnpm view only supports registry packages.`)
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
  const getAuthHeader = createGetAuthHeaderByURI(opts.authInfos ?? {}, opts.registries?.default)
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

  const versionsCount = metadata.versions ? Object.keys(metadata.versions).length : 0
  const depsCount = data.dependencies ? Object.keys(data.dependencies).length : 0
  const distTags = metadata['dist-tags']

  const info = {
    ...data,
    author: typeof data.author === 'object' ? (data.author as { name: string }).name : data.author,
    repository: typeof data.repository === 'object' ? data.repository.url : data.repository,
    versionsCount: versionsCount > 0 ? versionsCount : undefined,
    depsCount: depsCount > 0 ? depsCount : undefined,
    distTags,
  }

  // If fields are specified, filter and return only those
  if (fields.length > 0) {
    const selectedFields: Record<string, unknown> = {}
    for (const field of fields) {
      selectedFields[field] = getNestedProperty(info as unknown as Record<string, unknown>, field)
    }

    if (opts.json) {
      return JSON.stringify(selectedFields, null, 2)
    }

    if (fields.length === 1) {
      const value = selectedFields[fields[0]]
      return formatFieldValue(value)
    }

    const lines = fields.map(field => {
      const value = selectedFields[field]
      if (typeof value === 'object' && value !== null) {
        return `${field} = ${JSON.stringify(value)}`
      }
      if (typeof value === 'string') {
        return `${field} = '${value}'`
      }
      return `${field} = ${formatFieldValue(value)}`
    })
    return lines.join('\n')
  }

  if (opts.json) {
    return JSON.stringify(info, null, 2)
  }

  const headerParts: string[] = []

  if (info.name && info.version) {
    headerParts.push(`${info.name}@${info.version}`)
  }

  if (info.license) {
    headerParts.push(info.license)
  }

  if (info.depsCount !== undefined) {
    headerParts.push(`deps: ${info.depsCount}`)
  } else {
    headerParts.push('deps: none')
  }

  if (info.versionsCount !== undefined) {
    headerParts.push(`versions: ${info.versionsCount}`)
  }

  const lines = [headerParts.join(' | ')]

  if (info.description) {
    lines.push(info.description)
  }

  if (info.homepage) {
    lines.push(info.homepage)
  }

  if (info.keywords && info.keywords.length > 0) {
    lines.push('')
    lines.push(`keywords: ${info.keywords.join(', ')}`)
  }

  if (info.dependencies && Object.keys(info.dependencies).length > 0) {
    lines.push('')
    lines.push('dependencies:')
    const depEntries = Object.entries(info.dependencies).map(([name, version]) => `${name}: ${version}`)
    lines.push(depEntries.join(', '))
  }

  if (info.dist) {
    lines.push('')
    lines.push('dist')
    if (info.dist.tarball) {
      lines.push(`.tarball: ${info.dist.tarball}`)
    }
    if (info.dist.shasum) {
      lines.push(`.shasum: ${info.dist.shasum}`)
    }
    if (info.dist.integrity) {
      lines.push(`.integrity: ${info.dist.integrity}`)
    }
    if (info.dist.unpackedSize != null) {
      lines.push(`.unpackedSize: ${formatBytes(info.dist.unpackedSize)}`)
    }
  }

  if (info.maintainers && info.maintainers.length > 0) {
    lines.push('')
    lines.push('maintainers:')
    for (const maintainer of info.maintainers) {
      lines.push(`- ${maintainer.name}`)
    }
  }

  if (info.distTags && Object.keys(info.distTags).length > 0) {
    lines.push('')
    lines.push('dist-tags:')
    for (const [tag, tagVersion] of Object.entries(info.distTags)) {
      lines.push(`${tag}: ${tagVersion}`)
    }
  }

  return lines.join('\n')
}

function formatBytes (bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'kB', 'MB', 'GB', 'TB', 'PB']
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1)
  return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i]
}

function getNestedProperty (obj: Record<string, unknown>, path: string): unknown {
  return path.split('.').reduce((acc: unknown, part) => {
    if (typeof acc === 'object' && acc !== null) {
      return (acc as Record<string, unknown>)[part]
    }
    return undefined
  }, obj)
}

function formatFieldValue (value: unknown): string {
  if (value === null || value === undefined) {
    return ''
  }
  if (typeof value === 'object') {
    return JSON.stringify(value, null, 2)
  }
  return String(value)
}
