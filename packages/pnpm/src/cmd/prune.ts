import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import R = require('ramda')
import renderHelp = require('render-help')
import { InstallOptions, mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { readImporterManifestOnly } from '../readImporterManifest'
import { PnpmOptions } from '../types'
import { UNIVERSAL_OPTIONS } from './help'

export function types () {
  return R.pick([
    'production',
  ], allTypes)
}

export const commandNames = ['prune']

export function help () {
  return renderHelp({
    description: 'Removes extraneous packages',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Remove the packages specified in \`devDependencies\`',
            name: '--prod, --production',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('prune'),
    usages: ['pnpm prune [--production]'],
  })
}

export async function handler (input: string[], opts: PnpmOptions) {
  const store = await createStoreController(opts)
  return mutateModules([
    {
      buildIndex: 0,
      manifest: await readImporterManifestOnly(process.cwd(), opts),
      mutation: 'install',
      pruneDirectDependencies: true,
      rootDir: process.cwd(),
    },
  ], {
    ...opts,
    pruneStore: true,
    storeController: store.ctrl,
    storeDir: store.dir,
  } as InstallOptions)
}
