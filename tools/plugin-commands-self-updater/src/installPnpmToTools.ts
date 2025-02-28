import fs from 'fs'
import path from 'path'
import { getCurrentPackageName } from '@pnpm/cli-meta'
import { add } from '@pnpm/plugin-commands-installation'
import { getToolDirPath } from '@pnpm/tools.path'
import { sync as rimraf } from '@zkochan/rimraf'
import { fastPathTemp as pathTemp } from 'path-temp'
import omit from 'ramda/src/omit'
import renameOverwrite from 'rename-overwrite'
import { type SelfUpdateCommandOptions } from './selfUpdate'

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export async function installPnpmToTools (pnpmVersion: string, opts: SelfUpdateCommandOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: currentPkgName,
      version: pnpmVersion,
    },
  })

  const binDir = path.join(dir, 'bin')
  const alreadyExisted = fs.existsSync(binDir)
  if (alreadyExisted) {
    return {
      alreadyExisted,
      baseDir: dir,
      binDir,
    }
  }
  const stage = pathTemp(dir)
  fs.mkdirSync(stage, { recursive: true })
  fs.writeFileSync(path.join(stage, 'package.json'), '{}')
  try {
    await add.handler(
      {
        // Ideally the config reader should ignore these settings when the dlx command is executed.
        // This is a temporary solution until "@pnpm/config" is refactored.
        ...omit([
          'workspaceDir',
          'rootProjectManifest',
          'symlink',
          // Options from root manifest
          'allowNonAppliedPatches',
          'allowedDeprecatedVersions',
          'configDependencies',
          'ignoredBuiltDependencies',
          'ignoredOptionalDependencies',
          'neverBuiltDependencies',
          'onlyBuiltDependencies',
          'onlyBuiltDependenciesFile',
          'overrides',
          'packageExtensions',
          'patchedDependencies',
          'peerDependencyRules',
          'supportedArchitectures',
        ], opts),
        dir: stage,
        lockfileDir: stage,
        // We want to avoid symlinks because of the rename step,
        // which breaks the junctions on Windows.
        nodeLinker: 'hoisted',
        onlyBuiltDependencies: ['@pnpm/exe'],
        // This won't be used but there is currently no way to skip the bin creation
        // and we can't create the bin shims in the pnpm home directory
        // because the stage directory will be renamed.
        bin: path.join(stage, 'bin'),
      },
      [`${currentPkgName}@${pnpmVersion}`]
    )
    renameOverwrite.sync(stage, dir)
  } catch (err: unknown) {
    try {
      rimraf(stage)
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
  return {
    alreadyExisted,
    baseDir: dir,
    binDir,
  }
}
