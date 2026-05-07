import { docsUrl } from '@pnpm/cli.utils'
import { install } from '@pnpm/installing.commands'
import { renderHelp } from 'render-help'

import type { PnpmOptions } from '../types.js'
import * as clean from './clean.js'

export const rcOptionsTypes = install.rcOptionsTypes

export const cliOptionsTypes = install.cliOptionsTypes

export const shorthands = install.shorthands

export const commandNames = ['ci', 'clean-install', 'ic', 'install-clean']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    aliases: ['clean-install', 'ic', 'install-clean'],
    description: 'Runs "pnpm clean" followed by "pnpm install --frozen-lockfile". Designed for CI/CD environments.',
    url: docsUrl('ci'),
    usages: ['pnpm ci'],
  })
}

export async function handler (opts: PnpmOptions): Promise<void> {
  await clean.handler(opts)
  await install.handler({ ...opts, frozenLockfile: true } as any) // eslint-disable-line
}
