import fs from 'fs'
import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import { LogBase } from '@pnpm/logger'
import {
  CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import tempy from 'tempy'
import { PnpmError } from '@pnpm/error'
import { writePackage } from './writePackage'

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
          description: 'The package that needs to be modified will be extracted to this directory',
          name: '--edit-dir',
        },
      ],
    }],
    url: docsUrl('patch'),
    usages: ['pnpm patch <pkg name>@<version>'],
  })
}

export type PatchCommandOptions = Pick<Config, 'dir' | 'registries' | 'tag' | 'storeDir'> & CreateStoreControllerOptions & {
  editDir?: string
  reporter?: (logObj: LogBase) => void
}

export async function handler (opts: PatchCommandOptions, params: string[]) {
  if (opts.editDir && fs.existsSync(opts.editDir) && fs.readdirSync(opts.editDir).length > 0) {
    throw new PnpmError('PATCH_EDIT_DIR_EXISTS', `The target directory already exists: '${opts.editDir}'`)
  }
  const editDir = opts.editDir ?? tempy.directory()
  await writePackage(params[0], editDir, opts)
  return `You can now edit the following folder: ${editDir}`
}
