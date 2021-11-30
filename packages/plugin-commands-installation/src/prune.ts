import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { InstallOptions, mutateModules } from '@pnpm/core'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import getOptionsFromRootManifest from './getOptionsFromRootManifest'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return pick([
    'dev',
    'optional',
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
            description: 'Remove the packages specified in `devDependencies`',
            name: '--prod',
          },
          {
            description: 'Remove the packages specified in `optionalDependencies`',
            name: '--no-optional',
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
  opts: Pick<Config, 'dev' | 'engineStrict' | 'optional' | 'production' | 'rootProjectManifest'> & CreateStoreControllerOptions
) {
  const store = await createOrConnectStoreController(opts)
  const manifest = await readProjectManifestOnly(process.cwd(), opts)
  return mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      pruneDirectDependencies: true,
      rootDir: process.cwd(),
    },
  ], {
    ...opts,
    ...getOptionsFromRootManifest(opts.rootProjectManifest ?? {}),
    include: {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    },
    modulesCacheMaxAge: 0,
    pruneStore: true,
    storeController: store.ctrl,
    storeDir: store.dir,
  } as InstallOptions)
}
