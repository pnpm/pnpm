import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import open from 'open'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { fetchPackageInfo } from '../fetchPackageInfo.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(['registry'], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return rcOptionsTypes()
}

export const commandNames = ['docs', 'home']

export function help (): string {
  return renderHelp({
    description: 'Open the documentation of a package.',
    usages: [
      'pnpm docs <package-name>',
      'pnpm docs <package-name>@<version>',
      'pnpm home <package-name>',
    ],
  })
}

export async function handler (
  opts: Config & ConfigContext,
  params: string[]
): Promise<void> {
  const packageSpec = params[0]

  if (!packageSpec) {
    throw new PnpmError('MISSING_PACKAGE_NAME', 'Package name is required. Usage: pnpm docs <package-name>')
  }

  const info = await fetchPackageInfo(opts, packageSpec)

  const url = isHttpUrl(info.homepage) ? info.homepage : `https://npmx.dev/package/${info.name}`

  await open(url)
}

function isHttpUrl (value: unknown): value is string {
  if (typeof value !== 'string' || value === '') return false
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}
