import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import renderHelp from 'render-help'
import { createProjectManifestWriter } from './createProjectManifestWriter'
import * as install from './install'

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
  if (!opts.rootProjectManifest?.pnpm?.overrides) return 'Nothing to unlink'

  if (!params || (params.length === 0)) {
    for (const selector in opts.rootProjectManifest.pnpm.overrides) {
      if (opts.rootProjectManifest.pnpm.overrides[selector].startsWith('link:')) {
        delete opts.rootProjectManifest.pnpm.overrides[selector]
      }
    }
  } else {
    for (const selector in opts.rootProjectManifest.pnpm.overrides) {
      if (opts.rootProjectManifest.pnpm.overrides[selector].startsWith('link:') && params.includes(selector)) {
        delete opts.rootProjectManifest.pnpm.overrides[selector]
      }
    }
  }
  const writeProjectManifest = await createProjectManifestWriter(opts.rootProjectManifestDir)
  await writeProjectManifest(opts.rootProjectManifest)
  await install.handler(opts)
  return undefined
}
