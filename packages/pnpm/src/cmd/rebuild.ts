import { docsUrl, readImporterManifestOnly } from '@pnpm/cli-utils'
import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { createOrConnectStoreController } from '@pnpm/store-connection-manager'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  rebuild,
  rebuildPkgs,
} from 'supi'
import { PnpmOptions } from '../types'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    ...R.pick([
      'recursive',
    ], allTypes),
    'pending': Boolean,
  }
}

export const commandNames = ['rebuild', 'rb']

export function help () {
  return renderHelp({
    aliases: ['rb'],
    description: 'Rebuild a package.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`Rebuild every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Rebuild packages that were not build during installation. Packages are not build when installing with the --ignore-scripts flag',
            name: '--pending',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('rebuild'),
    usages: ['pnpm rebuild [<pkg> ...]'],
  })
}

export async function handler (
  args: string[],
  opts: PnpmOptions & { pending: boolean },
  command: string,
) {
  const store = await createOrConnectStoreController(opts)
  const rebuildOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (args.length === 0) {
    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readImporterManifestOnly(rebuildOpts.dir, opts),
          rootDir: rebuildOpts.dir,
        },
      ],
      rebuildOpts,
    )
  }
  await rebuildPkgs(
    [
      {
        manifest: await readImporterManifestOnly(rebuildOpts.dir, opts),
        rootDir: rebuildOpts.dir,
      },
    ],
    args,
    rebuildOpts,
  )
}
