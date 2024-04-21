import path from 'path'
import pnpmExec from '@pnpm/exec'
import { createExportableManifest } from '@pnpm/exportable-manifest'
import {
  getLockfileImporterId,
  type ProjectSnapshot,
  readWantedLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import { pruneSharedLockfile } from '@pnpm/prune-lockfile'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import pickBy from 'ramda/src/pickBy'
import renameOverwrite from 'rename-overwrite'

export async function makeDedicatedLockfile (lockfileDir: string, projectDir: string): Promise<void> {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  if (lockfile == null) {
    throw new Error('no lockfile found')
  }
  const allImporters = lockfile.importers
  lockfile.importers = {}
  const baseImporterId = getLockfileImporterId(lockfileDir, projectDir)
  for (const [importerId, importer] of Object.entries(allImporters)) {
    if (importerId.startsWith(`${baseImporterId}/`)) {
      const newImporterId = importerId.slice(baseImporterId.length + 1)
      lockfile.importers[newImporterId] = projectSnapshotWithoutLinkedDeps(importer)
      continue
    }
    if (importerId === baseImporterId) {
      lockfile.importers['.'] = projectSnapshotWithoutLinkedDeps(importer)
    }
  }
  const dedicatedLockfile = pruneSharedLockfile(lockfile)

  await writeWantedLockfile(projectDir, dedicatedLockfile)

  const { manifest, writeProjectManifest } = await readProjectManifest(projectDir)
  const publishManifest = await createExportableManifest(projectDir, manifest)
  await writeProjectManifest(publishManifest)

  const modulesDir = path.join(projectDir, 'node_modules')
  const tmp = path.join(projectDir, 'tmp_node_modules')
  const tempModulesDir = path.join(projectDir, 'node_modules/.tmp')
  let modulesRenamed = false
  try {
    await renameOverwrite(modulesDir, tmp)
    await renameOverwrite(tmp, tempModulesDir)
    modulesRenamed = true
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
  }

  try {
    await pnpmExec([
      'install',
      '--frozen-lockfile',
      '--lockfile-dir=.',
      '--fix-lockfile',
      '--filter=.',
      '--no-link-workspace-packages',
      '--config.dedupe-peer-dependents=false', // TODO: remove this. It should work without it
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

function projectSnapshotWithoutLinkedDeps (projectSnapshot: ProjectSnapshot): ProjectSnapshot {
  const newProjectSnapshot: ProjectSnapshot = {
    specifiers: projectSnapshot.specifiers,
  }
  for (const depField of DEPENDENCIES_FIELDS) {
    if (projectSnapshot[depField] == null) continue
    newProjectSnapshot[depField] = pickBy((depVersion) => !depVersion.startsWith('link:'), projectSnapshot[depField])
  }
  return newProjectSnapshot
}
