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
import PnpmError from '@pnpm/error'

export function rcOptionsTypes () {
  return pick([], allTypes)
}

export function cliOptionsTypes () {
  return { ...rcOptionsTypes(), 'edit-dir': String }
}

export const shorthands = {
  d: '--edit-dir',
}

export const commandNames = ['patch']

export function help () {
  return renderHelp({
    description: 'Prepare a package for patching',
    descriptionLists: [{
      title: 'Options',
      list: [
        {
          description: 'Directory path that patch files will be stored. If not set, the files is stored in temporary directory.',
          name: '--edit-dir',
          shortAlias: '-d',
        },
      ],
    }],
    url: docsUrl('patch'),
    usages: ['pnpm patch [--edit-dir <user directory path for patch>]'],
  })
}

export type PatchCommandOptions = Pick<
Config,
'dir' | 'registries' | 'tag' | 'storeDir'
> &
CreateStoreControllerOptions & {
  editDir?: string
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
    registry:
      (dep.alias && pickRegistryForPackage(opts.registries, dep.alias)) ??
      opts.registries.default,
  })
  const filesResponse = await pkgResponse.files!()

  const tempDir = tempy.directory()
  const patchPackageDir = createPackageDirectory(opts.editDir) ?? tempDir
  const userChangesDir = path.join(patchPackageDir, 'user')
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

function createPackageDirectory (editDir?: string) {
  if (!editDir) {
    return
  }

  if (fs.existsSync(editDir)) {
    throw new PnpmError('DIR_ALREADY_EXISTS', `The package directory already exists: '${editDir}'`)
  }

  fs.mkdirSync(editDir, { recursive: true })
  return editDir
}
