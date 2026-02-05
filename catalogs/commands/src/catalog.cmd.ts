import { docsUrl } from '@pnpm/cli-utils'
import { type Config } from '@pnpm/config'
import renderHelp from 'render-help'
import * as catalogMigrate from './catalogMigrate.cmd.js'
import { PnpmError } from '@pnpm/error'

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...catalogMigrate.rcOptionsTypes,
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...catalogMigrate.cliOptionsTypes(),
  }
}

export const commandNames = ['catalog']

export function help (): string {
  return renderHelp({
    description: 'Manage and maintain catalogs',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: 'Migrates dependencies to using catalogs',
            name: 'migrate',
          },
        ],
      },
    ],
    url: docsUrl('catalogs'),
    usages: ['pnpm catalog <command>'],
  })
}

export type CatalogCommandOptions = Pick<Config, 'cliOptions'> & {
  interactive?: boolean
}

export async function handler (opts: CatalogCommandOptions, params: string[]): Promise<string | undefined> {
  if (params.length === 0) {
    throw new PnpmError('CATALOG_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  switch (params[0]) {
  case 'migrate':
    return catalogMigrate.handler(opts as catalogMigrate.CatalogMigrateCommandOptions, params.slice(1))
  default:
    throw new PnpmError('CATALOG_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
}
