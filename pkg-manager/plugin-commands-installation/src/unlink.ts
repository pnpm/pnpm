import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { writeSettings } from '@pnpm/config.config-writer'
import renderHelp from 'render-help'
import * as install from './install.js'

export const cliOptionsTypes = install.cliOptionsTypes

export const rcOptionsTypes = install.rcOptionsTypes

export const commandNames = ['unlink', 'dislink']

export function help (): string {
  return renderHelp({
    aliases: ['dislink'],
    description: 'Removes the link created by `pnpm link` and reinstalls package if it is saved in `package.json`',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Unlink in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('unlink'),
    usages: [
      'pnpm unlink (in package dir)',
      'pnpm unlink <pkg>...',
    ],
  })
}

export async function handler (
  opts: install.InstallCommandOptions,
  params: string[]
): Promise<undefined | string> {
  if (!opts.overrides) return 'Nothing to unlink'

  if (!params || (params.length === 0)) {
    for (const selector in opts.overrides) {
      if (opts.overrides[selector].startsWith('link:')) {
        delete opts.overrides[selector]
      }
    }
  } else {
    for (const selector in opts.overrides) {
      if (opts.overrides[selector].startsWith('link:') && params.includes(selector)) {
        delete opts.overrides[selector]
      }
    }
  }
  await writeSettings({
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    rootProjectManifestDir: opts.rootProjectManifestDir,
    updatedSettings: {
      overrides: opts.overrides,
    },
  })
  await install.handler(opts)
  return undefined
}
