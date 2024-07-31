import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, getOptionsFromRootManifest } from '@pnpm/config'
import { createOrConnectStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { type InstallOptions, mutateModulesInSingleProject } from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import renderHelp from 'render-help'
import { cliOptionsTypes } from './install'

export const rcOptionsTypes = cliOptionsTypes

export { cliOptionsTypes }

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['fetch']

export function help (): string {
  return renderHelp({
    description: 'Fetch packages from a lockfile into virtual store, package manifest is ignored. WARNING! This is an experimental command. Breaking changes may be introduced in non-major versions of the CLI',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Only development packages will be fetched',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Development packages will not be fetched',
            name: '--prod',
            shortAlias: '-P',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('fetch'),
    usages: ['pnpm fetch [--dev | --prod]'],
  })
}

type FetchCommandOptions = Pick<Config, 'production' | 'dev' | 'rootProjectManifest' | 'rootProjectManifestDir'> & CreateStoreControllerOptions

export async function handler (opts: FetchCommandOptions): Promise<void> {
  const store = await createOrConnectStoreController(opts)
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    // when including optional deps, production is also required when perform headless install
    optionalDependencies: opts.production !== false,
  }
  await mutateModulesInSingleProject({
    manifest: {},
    mutation: 'install',
    pruneDirectDependencies: true,
    rootDir: process.cwd() as ProjectRootDir,
  }, {
    ...opts,
    ...getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest ?? {}),
    ignorePackageManifest: true,
    include,
    modulesCacheMaxAge: 0,
    pruneStore: true,
    storeController: store.ctrl,
    storeDir: store.dir,
  } as InstallOptions)
}
