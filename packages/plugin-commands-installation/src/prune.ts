import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import R = require('ramda')
import renderHelp = require('render-help')
import { InstallOptions, mutateModules } from 'supi'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
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
            name: '--prod',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('prune'),
    usages: ['pnpm prune [--prod]'],
  })
}

export async function handler (
  input: string[],
  opts: Pick<Config, 'dev' | 'engineStrict' | 'optional' | 'production'> & CreateStoreControllerOptions,
) {
  const store = await createOrConnectStoreController(opts)
  return mutateModules([
    {
      buildIndex: 0,
      manifest: await readProjectManifestOnly(process.cwd(), opts),
      mutation: 'install',
      pruneDirectDependencies: true,
      rootDir: process.cwd(),
    },
  ], {
    ...opts,
    include: {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    },
    pruneStore: true,
    storeController: store.ctrl,
    storeDir: store.dir,
  } as InstallOptions)
}
