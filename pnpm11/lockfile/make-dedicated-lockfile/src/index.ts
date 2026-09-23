import path from 'node:path'

import { pnpmExec } from '@pnpm/exec'
import {
  getLockfileImporterId,
  readWantedLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import { createExportableManifest } from '@pnpm/releasing.exportable-manifest'
import { DEPENDENCIES_FIELDS, type ProjectId, type ProjectManifest } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { renameOverwrite } from 'rename-overwrite'

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
      const newImporterId = importerId.slice(baseImporterId.length + 1) as ProjectId
      lockfile.importers[newImporterId] = importer
      continue
    }
    if (importerId === baseImporterId) {
      lockfile.importers['.' as ProjectId] = importer
    }
  }
  const dedicatedLockfile = pruneSharedLockfile(lockfile)

  await writeWantedLockfile(projectDir, dedicatedLockfile)

  const { manifest, writeProjectManifest } = await readProjectManifest(projectDir)
  const publishManifest = await createExportableManifest(projectDir, manifest, {
    // Since @pnpm/lockfile.make-dedicated-lockfile is deprecated, avoid supporting new
    // features like pnpm catalogs. Passing in an empty catalog object
    // intentionally.
    catalogs: {},
  })
  await writeProjectManifest(withWorkspaceDependencies(manifest, publishManifest as ProjectManifest))

  const modulesDir = path.join(projectDir, 'node_modules')
  const tempModulesDir = path.join(projectDir, '.tmp_node_modules')
  let modulesRenamed = false
  try {
    await renameOverwrite(modulesDir, tempModulesDir)
    modulesRenamed = true
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
  }

  let installError: unknown
  try {
    await pnpmExec([
      'install',
      '--frozen-lockfile',
      '--lockfile-dir=.',
      '--fix-lockfile',
      '--filter=.',
      '--config.dedupe-peer-dependents=false', // TODO: remove this. It should work without it
    ], {
      cwd: projectDir,
    })
  } catch (err) {
    installError = err
  }
  let restoreError: unknown
  if (modulesRenamed) {
    try {
      await renameOverwrite(tempModulesDir, modulesDir)
    } catch (err) {
      restoreError = err
    }
  }
  await writeProjectManifest(manifest)
  if (installError != null) {
    throw installError
  }
  if (restoreError != null) {
    throw restoreError
  }
}

/**
 * The exportable manifest replaces `workspace:` specifiers with the version
 * range a published copy would use, which the install would then look up in
 * the registry. The dedicated lockfile keeps the workspace project linked, so
 * its specifier stays as declared.
 */
function withWorkspaceDependencies (manifest: ProjectManifest, publishManifest: ProjectManifest): ProjectManifest {
  const result = { ...publishManifest }
  for (const depField of [...DEPENDENCIES_FIELDS, 'peerDependencies'] as const) {
    const deps = manifest[depField]
    if (deps == null || result[depField] == null) continue
    let updatedDeps: Record<string, string> | undefined
    for (const [depName, spec] of Object.entries(deps)) {
      const isWorkspace = depField === 'peerDependencies' ? spec.includes('workspace:') : spec.startsWith('workspace:')
      if (isWorkspace) {
        if (updatedDeps == null) {
          updatedDeps = { ...result[depField] }
        }
        updatedDeps[depName] = spec
      }
    }
    if (updatedDeps != null) {
      result[depField] = updatedDeps
    }
  }
  return result
}
