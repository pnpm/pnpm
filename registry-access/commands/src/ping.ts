import util from 'node:util'

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

export const commandNames = ['ping']

export function help (): string {
  return renderHelp({
    description: 'Test connectivity to the configured registry.',
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
  })
}

export async function handler (opts: PingOptions): Promise<string> {
  const registryUrl = opts.registry ?? opts.registries?.default ?? 'https://registry.npmjs.org/'
  const pingUrl = new URL('-/ping?write=true', registryUrl).toString()

  const fetchFromRegistry = createFetchFromRegistry(opts)
  const start = Date.now()
  let details = ''
  try {
    const response = await fetchFromRegistry(pingUrl, { retry: { retries: 0 } })
    const body = await response.text()
    if (body) {
      try {
        const parsed = JSON.parse(body)
        if (parsed && typeof parsed === 'object' && Object.keys(parsed).length > 0) {
          details = JSON.stringify(parsed, null, 2)
        }
      } catch {
        // non-JSON body — ignore
      }
    }
  } catch (err: unknown) {
    const errorMessage = util.types.isNativeError(err) ? err.message : String(err)
    throw new PnpmError('PING_ERROR', `Failed to reach registry: ${errorMessage}`)
  }
  const time = Date.now() - start

  const lines = [`PING ${registryUrl}`, `PONG ${time}ms`]
  if (details) lines.push(`PONG ${details}`)
  return lines.join('\n')
}
