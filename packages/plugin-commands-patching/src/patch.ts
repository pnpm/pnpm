import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import { LogBase } from '@pnpm/logger'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import storePath from '@pnpm/store-path'
import pick from 'ramda/src/pick'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import renderHelp from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return pick([
    'registry',
    'store',
    'store-dir',
  ], allTypes)
}

export const commandNames = ['patch']

export function help () {
  return renderHelp({
    description: 'Reads and performs actions on pnpm store that is on the current filesystem.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: '\
Checks for modified packages in the store. \
Returns exit code 0 if the content of the package is the same as it was at the time of unpacking',
            name: 'status',
          },
          {
            description: 'Adds new packages to the store. Example: pnpm store add express@4 typescript@2.1.0',
            name: 'add <pkg>...',
          },
          {
            description: '\
Removes unreferenced (extraneous, orphan) packages from the store. \
Pruning the store is not harmful, but might slow down future installations. \
Visit the documentation for more information on unreferenced packages and why they occur',
            name: 'prune',
          },
          {
            description: 'Returns the path to the active store directory.',
            name: 'path',
          },
        ],
      },
    ],
    url: docsUrl('store'),
    usages: ['pnpm store <command>'],
  })
}

export type PatchCommandOptions = Pick<Config, 'dir' | 'registries' | 'tag' | 'storeDir'> & CreateStoreControllerOptions & {
  reporter?: (logObj: LogBase) => void
}

export async function handler (opts: PatchCommandOptions, params: string[]) {
  const store = await createOrConnectStoreController({
    ...opts,
    packageImportMethod: 'clone-or-copy',
  })
  const dep = parseWantedDependency(params[0])
  const pkgResponse = await store.ctrl.requestPackage(dep, {
    downloadPriority: 1,
    lockfileDir: opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
    registry: (dep.alias && pickRegistryForPackage(opts.registries, dep.alias)) ?? opts.registries.default,
  })
  const filesResponse = await pkgResponse.files!()
  const tempDir = path.join(await getStoreTempDir(opts), Math.random().toString())
  const userChangesDir = path.join(tempDir, 'user')
  await Promise.all([
    store.ctrl.importPackage(path.join(tempDir, 'source'), {
      filesResponse,
      force: true,
    }),
    store.ctrl.importPackage(userChangesDir, {
      filesResponse,
      force: true,
    }),
  ])
  return `You can now edit the following folder: ${userChangesDir}`
}

async function getStoreTempDir (
  opts: {
    dir: string
    storeDir?: string
    pnpmHomeDir: string
  }
): Promise<string> {
  const storeDir = await storePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  return path.join(storeDir, 'tmp')
}
