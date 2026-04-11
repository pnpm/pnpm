import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(['registry'], allTypes)
}

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
  date: string
  author?: { name: string } | string
  publisher?: { username: string }
  maintainers?: Array<{ username: string }>
}

export interface SearchResult {
  package: SearchPackage
}

export async function handler (
  opts: Config & ConfigContext & {
    json?: boolean
    searchLimit?: number
  },
  params: string[]
): Promise<string | void> {
  const query = params.join(' ')

  if (!query) {
    throw new PnpmError('MISSING_SEARCH_QUERY', 'Search query is required. Usage: pnpm search <keyword>')
  }

  const registry = opts.registries?.default || 'https://registry.npmjs.org/'
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, opts.registries?.default)

  const searchUrl = new URL(registry)
  searchUrl.pathname = '/-/v1/search'
  searchUrl.searchParams.set('text', query)
  searchUrl.searchParams.set('size', (opts.searchLimit ?? 20).toString())

  const response = await fetchFromRegistry(searchUrl.toString(), {
    authHeaderValue: getAuthHeader(registry),
  })

  if (!response.ok) {
    throw new PnpmError('SEARCH_FAILED', `Search failed with status ${response.status}: ${response.statusText}`)
  }

  const data = await response.json() as { objects: SearchResult[] }

  if (opts.json) {
    return JSON.stringify(data.objects.map(obj => obj.package), null, 2)
  }

  if (data.objects.length === 0) {
    return 'No packages found'
  }

  const tableData = [
    [
      chalk.blueBright('Package'),
      chalk.blueBright('Description'),
      chalk.blueBright('Version'),
      chalk.blueBright('Date'),
      chalk.blueBright('Author'),
    ],
    ...data.objects.map(obj => {
      const pkg = obj.package
      return [
        chalk.bold(pkg.name),
        pkg.description || '',
        pkg.version,
        pkg.date ? pkg.date.split('T')[0] : '',
        typeof pkg.author === 'object' ? pkg.author.name : (pkg.author || pkg.publisher?.username || ''),
      ]
    }),
  ]

  return table(tableData, {
    columns: {
      1: {
        width: 40,
        wrapWord: true,
      },
    },
  })
}
