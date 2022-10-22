import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { logger } from '@pnpm/logger'
import { Config } from '@pnpm/config'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export const shorthands = {}

export const commandNames = ['doctor']

export function help () {
  return renderHelp({
    description: 'Checks for known common issues.',
    url: docsUrl('doctor'),
    usages: ['pnpm doctor [options]'],
  })
}

export async function handler (
  opts: Pick<Config, 'failedToLoadBuiltInConfig'>
) {
  const { failedToLoadBuiltInConfig } = opts
  if (failedToLoadBuiltInConfig) {
    // If true, means loading npm builtin config failed. Then there may have a prefix error, related: https://github.com/pnpm/pnpm/issues/5404
    logger.warn({
      message: 'Load npm builtin configs failed. If the prefix builtin config does not work, you can use "pnpm config ls" to show builtin configs. And then use "pnpm config --global set <key> <value>" to migrate configs from builtin to global.',
      prefix: process.cwd(),
    })
  }
}
