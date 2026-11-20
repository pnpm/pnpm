import type { CommandHandlerMap } from '@pnpm/cli.command'
import { docsUrl } from '@pnpm/cli.utils'
import { install, validateCiFrozenInstall } from '@pnpm/installing.commands'
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

export async function handler (
  opts: PnpmOptions,
  params?: string[],
  commands?: CommandHandlerMap
): Promise<void | string> {
  const skipsClean = Boolean(
    opts.dryRun ||
    opts.lockfileOnly ||
    (opts.cliOptions as { 'resolution-only'?: boolean } | undefined)?.['resolution-only']
  )
  let cleanPlanPromise: Promise<clean.CiCleanPlan> | undefined
  const getCleanPlan = async (): Promise<clean.CiCleanPlan> => {
    cleanPlanPromise ??= clean.prepareCiCleanPlan(opts)
    return cleanPlanPromise
  }
  const preparedCi = skipsClean
    ? undefined
    : await validateCiFrozenInstall({
      ...opts,
      _cleansModulesDir: async (anchorDir: string, modulesDir?: string) =>
        (await getCleanPlan()).cleansModulesDir(anchorDir, modulesDir),
      frozenLockfile: true,
    } as any) // eslint-disable-line
  try {
    if (!skipsClean) {
      // `--lockfile` here is the install setting, not `pnpm clean --lockfile`.
      await (await getCleanPlan()).execute()
    }
    return await install.handler(
      { ...opts, _preparedCi: preparedCi, frozenLockfile: true, optimisticRepeatInstall: false } as any, // eslint-disable-line
      params,
      commands
    )
  } finally {
    await preparedCi?.close()
  }
}
