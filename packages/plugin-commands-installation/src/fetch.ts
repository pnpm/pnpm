import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { InstallOptions, mutateModules } from 'supi'
import * as R from 'ramda'
import renderHelp from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'production',
    'dev',
  ], allTypes)
}

export const commandNames = ['fetch']

export function help () {
  return renderHelp({
    description: 'Fetch packages from a lockfile into virtual store, package manifest is ignored. WARNING! This is an experimental command. Breaking changes may be introduced in non-major versions of the CLI',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Only development packages will be fetched',
            name: '--dev',
          },
          {
            description: 'Development packages will not be fetched',
            name: '--prod',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('fetch'),
    usages: ['pnpm fetch [--dev | --prod]'],
  })
}

export async function handler (
  opts: Pick<Config, 'production' | 'dev'> & CreateStoreControllerOptions
) {
  const store = await createOrConnectStoreController(opts)
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    // when including optional deps, production is also required when perform headless install
    optionalDependencies: opts.production !== false,
  }
  return mutateModules([
    {
      buildIndex: 0,
      manifest: {},
      mutation: 'install',
      pruneDirectDependencies: true,
      rootDir: process.cwd(),
    },
  ], {
    ...opts,
    ignorePackageManifest: true,
    include,
    modulesCacheMaxAge: 0,
    pruneStore: true,
    storeController: store.ctrl,
    storeDir: store.dir,
  } as InstallOptions)
}
