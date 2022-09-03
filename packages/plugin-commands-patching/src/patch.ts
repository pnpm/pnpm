import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import { LogBase } from '@pnpm/logger'
import {
  createOrConnectStoreController,
  CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import pick from 'ramda/src/pick'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import renderHelp from 'render-help'
import tempy from 'tempy'

export function rcOptionsTypes () {
  return pick([], allTypes)
}

export function cliOptionsTypes () {
  return { ...rcOptionsTypes(), path: String }
}

export const shorthands = {
  p: '--path',
}

export const commandNames = ['patch']

export function help () {
  return renderHelp({
    description: 'Prepare a package for patching',
    descriptionLists: [{
      title: 'Options',
      list: [
        {
          description: 'set a package directory path for patching',
          name: '--path',
          shortAlias: '-p',
        },
      ],
    }],
    url: docsUrl('patch'),
    usages: ['pnpm patch [--path <patch directory path>]'],
  })
}

export type PatchCommandOptions = Pick<
Config,
'dir' | 'registries' | 'tag' | 'storeDir'
> &
CreateStoreControllerOptions & {
  path?: string
  reporter?: (logObj: LogBase) => void
}

/**
 * @TODO
 * 3. document 업데이트
 */
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
    registry:
      (dep.alias && pickRegistryForPackage(opts.registries, dep.alias)) ??
      opts.registries.default,
  })
  const filesResponse = await pkgResponse.files!()

  const patchPackageDir = createPackageDirectory(opts.path)
  const userChangesDir = path.join(patchPackageDir, 'user')
  await Promise.all([
    store.ctrl.importPackage(path.join(patchPackageDir, 'source'), {
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

function createPackageDirectory (userDir?: string) {
  if (!userDir) {
    return tempy.directory()
  }

  if (fs.existsSync(userDir)) {
    throw new Error(`The package directory already exists: '${userDir}'`)
  }

  fs.mkdirSync(userDir, { recursive: true })
  return userDir
}