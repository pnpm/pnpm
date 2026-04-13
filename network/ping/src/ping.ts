import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import type { Registries } from '@pnpm/types'
import { renderHelp } from 'render-help'

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    registry: String,
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export interface PingOptions extends CreateFetchFromRegistryOptions {
  registry?: string
  registries?: Registries
}

export const ping = {
  cliOptionsTypes,
  commandNames: ['ping'],
  handler: pingHandler,
  help: (): string => renderHelp({
    description: 'Test connectivity to configured registries.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Test a specific registry URL',
            name: '--registry <url>',
          },
        ],
      },
    ],
    url: docsUrl('ping'),
    usages: ['pnpm ping [--registry <url>]'],
  }),
  rcOptionsTypes,
}

async function pingHandler (opts: PingOptions): Promise<string> {
  const registryUrl = opts.registry ?? opts.registries?.default ?? 'https://registry.npmjs.org/'

  try {
    const fetchFromRegistry = createFetchFromRegistry(opts)
    const response = await fetchFromRegistry(registryUrl, {
      method: 'HEAD',
    })

    if (!response.ok) {
      throw new PnpmError(
        'PING_FAILED',
        `Registry returned status ${response.status}: ${response.statusText}`
      )
    }

    return `Registry is reachable: ${registryUrl}`
  } catch (err: unknown) {
    if (err instanceof PnpmError) throw err
    const errorMessage = err instanceof Error ? err.message : String(err)
    throw new PnpmError('PING_ERROR', `Failed to reach registry: ${errorMessage}`)
  }
}
