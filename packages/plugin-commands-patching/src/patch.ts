import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import { LogBase } from '@pnpm/logger'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import pick from 'ramda/src/pick.js'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import renderHelp from 'render-help'
import tempy from 'tempy'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return pick([], allTypes)
}

export const commandNames = ['patch']

export function help () {
  return renderHelp({
    description: 'Prepare a package for patching',
    descriptionLists: [],
    url: docsUrl('patch'),
    usages: ['pnpm patch'],
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
  const tempDir = tempy.directory()
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
