import { docsUrl } from '@pnpm/cli-utils'
import { oneLine } from 'common-tags'
import renderHelp = require('render-help')
import { mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { readImporterManifestOnly } from '../readImporterManifest'
import { PnpmOptions } from '../types'
import { UNIVERSAL_OPTIONS } from './help'
import { types } from './install'

export { types }

export const commandNames = ['unlink', 'dislink']

export function help () {
  return renderHelp({
    aliases: ['dislink'],
    description: 'Removes the link created by \`pnpm link\` and reinstalls package if it is saved in \`package.json\`',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`
              Unlink in every package found in subdirectories
              or in every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
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

export async function handler (input: string[], opts: PnpmOptions) {
  const store = await createStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (!input || !input.length) {
    return mutateModules([
      {
        dependencyNames: input,
        manifest: await readImporterManifestOnly(opts.dir, opts),
        mutation: 'unlinkSome',
        rootDir: opts.dir,
      },
    ], unlinkOpts)
  }
  return mutateModules([
    {
      manifest: await readImporterManifestOnly(opts.dir, opts),
      mutation: 'unlink',
      rootDir: opts.dir,
    },
  ], unlinkOpts)
}
