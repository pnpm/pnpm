import { docsUrl } from '@pnpm/cli.utils'
import { run } from '@pnpm/exec.commands'
import { install } from '@pnpm/installing.commands'
import { renderHelp } from 'render-help'

import type { PnpmOptions } from '../types.js'

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...install.cliOptionsTypes(),
    bail: Boolean,
  }
}

export const rcOptionsTypes = install.rcOptionsTypes

export const commandNames = ['install-test', 'it']

export function help (): string {
  return renderHelp({
    aliases: ['it'],
    description: 'Runs a `pnpm install` followed immediately by a `pnpm test`. Accepts the same arguments as `pnpm install`, plus `--no-bail` to continue running workspace tests after a failure.',
    url: docsUrl('install-test'),
    usages: ['pnpm install-test'],
  })
}

export async function handler (opts: PnpmOptions, params: string[]): Promise<void> {
  await install.handler(opts)
  await run.handler(opts as any, ['test', ...params]) // eslint-disable-line
}
