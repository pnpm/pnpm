import pnpmExec from '@pnpm/exec'
import exportableManifest from '@pnpm/exportable-manifest'
import {
  getLockfileImporterId,
  readWantedLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import { pruneSharedLockfile } from '@pnpm/prune-lockfile'
import readProjectManifest from '@pnpm/read-project-manifest'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import fs = require('mz/fs')
import path = require('path')
import renameOverwrite = require('rename-overwrite')

export default async function (lockfileDir: string, projectDir: string) {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  if (!lockfile) {
    throw new Error('no lockfile found')
  }
  lockfile.importers = {
    '.': lockfile.importers[getLockfileImporterId(lockfileDir, projectDir)],
  }
  const dedicatedLockfile = pruneSharedLockfile(lockfile)

  for (const depField of DEPENDENCIES_FIELDS) {
    if (!dedicatedLockfile.importers['.'][depField]) continue
    for (const depName of Object.keys(dedicatedLockfile.importers['.'][depField]!)) {
      if (dedicatedLockfile.importers['.'][depField]![depName].startsWith('link:')) {
        delete dedicatedLockfile.importers['.'][depField]![depName]
      }
    }
  }

  await writeWantedLockfile(projectDir, dedicatedLockfile)
  const modulesDir = path.join(projectDir, 'node_modules')
  const tempModulesDir = path.join(projectDir, 'test/.tmp_node_modules')
  let modulesRenamed = false
  try {
    await renameOverwrite(modulesDir, tempModulesDir)
    modulesRenamed = true
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err
  }

  const { manifest, writeProjectManifest } = await readProjectManifest(projectDir)
  const publishManifest = await exportableManifest(projectDir, manifest)
  await writeProjectManifest(publishManifest)

  try {
    await pnpmExec([
      'install',
      '--lockfile-dir=.',
      '--lockfile-only',
      '--filter=.',
      '--no-link-workspace-packages',
    ], {
      cwd: projectDir,
    })
  } catch (err) {
    throw err
  } finally {
    if (modulesRenamed) {
      await renameOverwrite(tempModulesDir, modulesDir)
    }
    await writeProjectManifest(manifest)
  }
}
