import fs from 'node:fs'
import path from 'node:path'

import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { isWorkspaceProjectDir } from '@pnpm/workspace.package-patterns'
import { isSubdir } from 'is-subdir'

import type { AddCommandOptions } from './add.js'

const ROOT_WORKSPACE_ADD_MESSAGE =
  'Running this command will add the dependency to the workspace root, ' +
  'which might not be what you want - if you really meant it, ' +
  'make it explicit by running this command again with the -w flag (or --workspace-root). ' +
  'If you don\'t want to see this warning anymore, you may set the ignore-workspace-root-check setting to true.'

const WORKSPACE_PACKAGE_DIR_ADD_MESSAGE =
  'Running this command in a workspace package directory without a package manifest will add the dependency to the workspace root, which might not be what you want. ' +
  'If this directory is meant to be a package, create a package manifest first, for example by running "pnpm init" to generate a package.json. ' +
  'If you really meant it, make it explicit by running this command again with the -w flag (or --workspace-root). ' +
  'If you don\'t want to see this warning anymore, you may set the ignore-workspace-root-check setting to true.'

export function assertNotAddingToWorkspaceRoot (opts: AddCommandOptions): void {
  if (
    opts.recursive ||
    opts.workspaceDir !== opts.dir ||
    opts.ignoreWorkspaceRootCheck ||
    opts.workspaceRoot
  ) return
  if (isUninitializedWorkspaceProjectDir(opts)) {
    throw new PnpmError('ADDING_TO_ROOT', WORKSPACE_PACKAGE_DIR_ADD_MESSAGE)
  }
  if (opts.workspacePackagePatterns && opts.workspacePackagePatterns.length > 1) {
    throw new PnpmError('ADDING_TO_ROOT', ROOT_WORKSPACE_ADD_MESSAGE)
  }
}

/**
 * `pnpm add` run from a directory that the workspace patterns select but that
 * has no manifest yet falls back to the workspace root. Such a directory is
 * most likely a package that was not initialized.
 */
function isUninitializedWorkspaceProjectDir (opts: AddCommandOptions): boolean {
  const { workspaceDir } = opts
  if (!workspaceDir) return false
  const invocationDir = (opts.cliOptions.dir as string | undefined) ?? fs.realpathSync(process.cwd())
  if (invocationDir === workspaceDir) return false
  if (MANIFEST_BASE_NAMES.some((manifestFileName) => fs.existsSync(path.join(invocationDir, manifestFileName)))) return false
  const managedDirs = [opts.modulesDir, opts.storeDir, opts.cacheDir]
    .filter((dir): dir is string => dir != null)
    .map((dir) => path.resolve(workspaceDir, dir))
    .filter((managedDir) => !isSubdir(managedDir, workspaceDir))
  if (managedDirs.some((managedDir) => isSubdir(managedDir, invocationDir))) return false
  return isWorkspaceProjectDir({
    workspaceDir,
    dir: invocationDir,
    patterns: opts.workspacePackagePatterns ?? ['.'],
  })
}
