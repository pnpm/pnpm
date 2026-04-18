import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'
import chalk from 'chalk'
import { renderHelp } from 'render-help'

import { rcOptionsTypes } from './common.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    json: Boolean,
    'search-limit': Number,
  }
}

export const commandNames = ['search', 's', 'se', 'find']

export function help (): string {
  return renderHelp({
    description: 'Search for packages in the registry.',
    url: docsUrl('search'),
    usages: [
      'pnpm search <keyword> ...',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Show search results in JSON format',
            name: '--json',
          },
          {
            description: 'Maximum number of results to show (default: 20)',
            name: '--search-limit <number>',
          },
        ],
      },
    ],
  })
}

export interface SearchPackage {
  name: string
  version: string
  description?: string
  date?: string
  author?: { name: string } | string
  publisher?: { username: string }
  maintainers?: Array<{ username: string }>
  keywords?: string[]
}

export interface SearchResult {
  package: SearchPackage
}

export interface SearchOptions extends CreateFetchFromRegistryOptions {
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
  json?: boolean
  searchLimit?: number
}

export async function handler (
  opts: SearchOptions,
  params: string[]
): Promise<string> {
  const query = params.join(' ')

  if (!query) {
    throw new PnpmError('MISSING_SEARCH_QUERY', 'Search query is required. Usage: pnpm search <keyword>')
  }

  const registry = opts.registries?.default ?? 'https://registry.npmjs.org/'
  const registryUrl = new URL(registry)
  if (!registryUrl.pathname.endsWith('/')) {
    registryUrl.pathname = `${registryUrl.pathname}/`
  }
  const searchUrl = new URL('./-/v1/search', registryUrl)
  searchUrl.searchParams.set('text', query)
  searchUrl.searchParams.set('size', (opts.searchLimit ?? 20).toString())

  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, opts.registries?.default)

  const response = await fetchFromRegistry(searchUrl.toString(), {
    authHeaderValue: getAuthHeader(registry),
  })

  if (!response.ok) {
    const errorBody = (await response.text()).trim()
    throw new PnpmError(
      'SEARCH_FAILED',
      errorBody
        ? `Search failed with status ${response.status}: ${response.statusText}. ${errorBody}`
        : `Search failed with status ${response.status}: ${response.statusText}`
    )
  }

  const data = await response.json() as { objects: SearchResult[] }

  if (opts.json) {
    return JSON.stringify(data.objects.map(obj => obj.package), null, 2)
  }

  if (data.objects.length === 0) {
    return 'No packages found'
  }

  return data.objects.map(obj => formatPackage(obj.package)).join('\n\n')
}

function formatPackage (pkg: SearchPackage): string {
  const author = typeof pkg.author === 'object'
    ? pkg.author.name
    : (pkg.author ?? pkg.publisher?.username ?? '')
  const date = pkg.date ? pkg.date.split('T')[0] : ''
  const lines = [
    chalk.bold(pkg.name),
  ]
  if (pkg.description) {
    lines.push(pkg.description)
  }
  const versionLine = [`Version ${pkg.version}`]
  if (date) versionLine.push(`published ${date}`)
  if (author) versionLine.push(`by ${author}`)
  lines.push(versionLine.join(' '))
  if (pkg.maintainers?.length) {
    lines.push(`Maintainers: ${pkg.maintainers.map(m => m.username).join(', ')}`)
  }
  if (pkg.keywords?.length) {
    lines.push(`Keywords: ${pkg.keywords.join(', ')}`)
  }
  lines.push(chalk.blueBright(`https://npmx.dev/package/${pkg.name}`))
  return lines.join('\n')
}
