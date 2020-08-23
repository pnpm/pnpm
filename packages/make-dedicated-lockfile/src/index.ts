import pnpmExec from '@pnpm/exec'
import exportableManifest from '@pnpm/exportable-manifest'
import {
  getLockfileImporterId,
  ProjectSnapshot,
  readWantedLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import { pruneSharedLockfile } from '@pnpm/prune-lockfile'
import readProjectManifest from '@pnpm/read-project-manifest'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import path = require('path')
import R = require('ramda')
import renameOverwrite = require('rename-overwrite')

export default async function (lockfileDir: string, projectDir: string) {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  if (!lockfile) {
    throw new Error('no lockfile found')
  }
  const allImporters = lockfile.importers
  lockfile.importers = {}
  const baseImporterId = getLockfileImporterId(lockfileDir, projectDir)
  for (const [importerId, importer] of Object.entries(allImporters)) {
    if (importerId.startsWith(`${baseImporterId}/`)) {
      const newImporterId = importerId.substr(baseImporterId.length + 1)
      lockfile.importers[newImporterId] = projectSnapshotWithoutLinkedDeps(importer)
      continue
    }
    if (importerId === baseImporterId) {
      lockfile.importers['.'] = projectSnapshotWithoutLinkedDeps(importer)
    }
  }
  const dedicatedLockfile = pruneSharedLockfile(lockfile)

  await writeWantedLockfile(projectDir, dedicatedLockfile)
  const modulesDir = path.join(projectDir, 'node_modules')
  const tmp = path.join(projectDir, 'tmp_node_modules')
  const tempModulesDir = path.join(projectDir, 'node_modules/.tmp')
  let modulesRenamed = false
  try {
    await renameOverwrite(modulesDir, tmp)
    await renameOverwrite(tmp, tempModulesDir)
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
  } finally {
    if (modulesRenamed) {
      await renameOverwrite(tempModulesDir, tmp)
      await renameOverwrite(tmp, modulesDir)
    }
    await writeProjectManifest(manifest)
  }
}

function projectSnapshotWithoutLinkedDeps (projectSnapshot: ProjectSnapshot) {
  const newProjectSnapshot: ProjectSnapshot = {
    specifiers: projectSnapshot.specifiers,
  }
  for (const depField of DEPENDENCIES_FIELDS) {
    if (!projectSnapshot[depField]) continue
    newProjectSnapshot[depField] = R.fromPairs(
      Object.entries(projectSnapshot[depField]!)
        .filter(([depName, depVersion]) => !depVersion.startsWith('link:'))
    )
  }
  return newProjectSnapshot
}
