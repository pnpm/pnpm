import fs from 'fs'
import path from 'path'
import { applyPatchToDir } from '@pnpm/patching.apply-patch'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { type LogBase } from '@pnpm/logger'
import {
  type CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import chalk from 'chalk'
import terminalLink from 'terminal-link'
import tempy from 'tempy'
import { PnpmError } from '@pnpm/error'
import { type ParseWantedDependencyResult } from '@pnpm/parse-wanted-dependency'
import { writePackage } from './writePackage'
import { getPatchedDependency } from './getPatchedDependency'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return { ...rcOptionsTypes(), 'edit-dir': String, 'ignore-existing': Boolean }
}

export const shorthands = {
  d: '--edit-dir',
}

export const commandNames = ['patch']

export function help (): string {
  return renderHelp({
    description: 'Prepare a package for patching',
    descriptionLists: [{
      title: 'Options',
      list: [
        {
          description: 'The package that needs to be modified will be extracted to this directory',
          name: '--edit-dir',
        },
        {
          description: 'Ignore existing patch files when patching',
          name: '--ignore-existing',
        },
      ],
    }],
    url: docsUrl('patch'),
    usages: ['pnpm patch <pkg name>@<version>'],
  })
}

export type PatchCommandOptions = Pick<Config,
| 'dir'
| 'registries'
| 'tag'
| 'storeDir'
| 'rootProjectManifest'
| 'lockfileDir'
| 'modulesDir'
| 'virtualStoreDir'
| 'sharedWorkspaceLockfile'
> & CreateStoreControllerOptions & {
  editDir?: string
  reporter?: (logObj: LogBase) => void
  ignoreExisting?: boolean
}

export async function handler (opts: PatchCommandOptions, params: string[]): Promise<string> {
  if (opts.editDir && fs.existsSync(opts.editDir) && fs.readdirSync(opts.editDir).length > 0) {
    throw new PnpmError('PATCH_EDIT_DIR_EXISTS', `The target directory already exists: '${opts.editDir}'`)
  }
  if (!params[0]) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm patch` requires the package name')
  }
  const editDir = opts.editDir ?? tempy.directory()
  const lockfileDir = opts.lockfileDir ?? opts.dir ?? process.cwd()
  const patchedDep = await getPatchedDependency(params[0], {
    lockfileDir,
    modulesDir: opts.modulesDir,
    virtualStoreDir: opts.virtualStoreDir,
  })

  await writePackage(patchedDep, editDir, opts)

  if (!opts.ignoreExisting) {
    let rootProjectManifest = opts.rootProjectManifest
    if (!opts.sharedWorkspaceLockfile) {
      const { manifest } = await tryReadProjectManifest(lockfileDir)
      if (manifest) {
        rootProjectManifest = manifest
      }
    }
    if (rootProjectManifest?.pnpm?.patchedDependencies) {
      tryPatchWithExistingPatchFile({
        patchedDep,
        patchedDir: editDir,
        patchedDependencies: rootProjectManifest.pnpm.patchedDependencies,
        lockfileDir,
      })
    }
  }
  return `Patch: You can now edit the package at:

  ${terminalLink(chalk.blue(editDir), 'file://' + editDir)}

To commit your changes, run:

  ${chalk.green(`pnpm patch-commit '${editDir}'`)}

`
}

function tryPatchWithExistingPatchFile (
  {
    patchedDep,
    patchedDir,
    patchedDependencies,
    lockfileDir,
  }: {
    patchedDep: ParseWantedDependencyResult
    patchedDir: string
    patchedDependencies: Record<string, string>
    lockfileDir: string
  }
): void {
  if (!patchedDep.alias || !patchedDep.pref) {
    return
  }
  const existingPatchFile = patchedDependencies[`${patchedDep.alias}@${patchedDep.pref}`]
  if (!existingPatchFile) {
    return
  }
  const existingPatchFilePath = path.resolve(lockfileDir, existingPatchFile)
  if (!fs.existsSync(existingPatchFilePath)) {
    throw new PnpmError('PATCH_FILE_NOT_FOUND', `Unable to find patch file ${existingPatchFilePath}`)
  }
  applyPatchToDir({ patchedDir, patchFilePath: existingPatchFilePath })
}
