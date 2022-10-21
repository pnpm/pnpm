import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { logger } from '@pnpm/logger'
import loadNpmConf from '@pnpm/npm-conf'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {
    config: Boolean,
  }
}

export const shorthands = {
  C: '--config',
}

export const commandNames = ['doctor']

export function help () {
  return renderHelp({
    description: 'Checks for known common issues.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Check the global config',
            name: '--config',
            shortAlias: '-C',
          },
        ],
      },
    ],
    url: docsUrl('doctor'),
    usages: ['pnpm doctor [options]'],
  })
}

export async function handler (
  opts: {
    config: boolean
  }) {
  if (opts.config) {
    const { failedToLoadBuiltInConfig } = loadNpmConf()
    if (
      // If error, means loading npm builtin config failed
      failedToLoadBuiltInConfig &&
      // Npm installed via brew may have prefix error, related: https://github.com/pnpm/pnpm/issues/5404
      process.platform === 'darwin' &&
      process.env.HOMEBREW_PREFIX &&
      process.execPath.startsWith(process.env.HOMEBREW_PREFIX)
    ) {
      logger.warn({
        message: 'Load npm builtin configs failed. If the prefix builtin config does not work, you can use "pnpm config ls" to show builtin configs. And then use "pnpm config --global set <key> <value>" to migrate configs from builtin to global.',
        prefix: process.cwd(),
      })
    }
  }
}
