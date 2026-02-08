import { docsUrl } from '@pnpm/cli-utils'
import { type Config } from '@pnpm/config'
import renderHelp from 'render-help'
import * as catalogMigrate from './catalogMigrate.cmd.js'
import { PnpmError } from '@pnpm/error'

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export const commandNames = ['catalog']

export const subcommands = [catalogMigrate]

export const description = 'Manage and maintain catalogs'

export function help (): string {
  return renderHelp({
    description,
    descriptionLists: [
      {
        title: 'Commands',

        list: subcommands.map((cmd) => ({
          name: cmd.commandNames.join(', '),
          description: cmd.description,
          shortAlias: undefined,
        })),
      },
    ],
    url: docsUrl('catalogs'),
    usages: ['pnpm catalog <command>'],
  })
}

export type CatalogCommandOptions = Pick<Config, 'cliOptions'>

export async function handler (opts: CatalogCommandOptions, params: string[]): Promise<string | undefined> {
  if (params.length === 0) {
    throw new PnpmError('CATALOG_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  throw new PnpmError('CATALOG_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
}
