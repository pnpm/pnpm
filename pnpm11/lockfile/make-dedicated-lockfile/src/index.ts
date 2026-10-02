import fs from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'
import { pnpmExec } from '@pnpm/exec'
import {
  getLockfileImporterId,
  type LockfileObject,
  readWantedLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import { createExportableManifest } from '@pnpm/releasing.exportable-manifest'
import { DEPENDENCIES_FIELDS, type ProjectId, type ProjectManifest } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { renameOverwrite } from 'rename-overwrite'

export async function makeDedicatedLockfile (lockfileDir: string, projectDir: string): Promise<void> {
  const tempModulesDir = path.join(projectDir, '.tmp_node_modules')
  if (fs.existsSync(tempModulesDir)) {
    throw new PnpmError('STAGED_MODULES_DIR_EXISTS', `${tempModulesDir} already exists`, {
      hint: 'It holds the node_modules of an earlier run that could not be moved back. Restore or remove it, then run the command again.',
    })
  }
  await writeDedicatedLockfile(lockfileDir, projectDir)

  const { manifest, writeProjectManifest } = await readProjectManifest(projectDir)
  const publishManifest = await createExportableManifest(projectDir, manifest, {
    // Since @pnpm/lockfile.make-dedicated-lockfile is deprecated, avoid supporting new
    // features like pnpm catalogs. Passing in an empty catalog object
    // intentionally.
    catalogs: {},
  })
  await writeProjectManifest(withWorkspaceDependencies(manifest, publishManifest as ProjectManifest))

  const modulesDir = path.join(projectDir, 'node_modules')
  const modulesRenamed = await stageModulesDir(modulesDir, tempModulesDir)

  const errors: unknown[] = []
  try {
    await installFromDedicatedLockfile(projectDir)
  } catch (err) {
    errors.push(err)
  }
  let modulesRestored = !modulesRenamed
  if (modulesRenamed) {
    try {
      await renameOverwrite(tempModulesDir, modulesDir)
      modulesRestored = true
    } catch (err) {
      errors.push(err)
    }
  }
  try {
    await writeProjectManifest(manifest)
  } catch (err) {
    errors.push(err)
  }
  throwCollectedErrors(errors, { projectDir, tempModulesDir, modulesRestored })
}

async function writeDedicatedLockfile (lockfileDir: string, projectDir: string): Promise<void> {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  if (lockfile == null) {
    throw new Error('no lockfile found')
  }
  lockfile.importers = pickProjectImporters(lockfile.importers, getLockfileImporterId(lockfileDir, projectDir))
  const dedicatedLockfile = pruneSharedLockfile(lockfile)

  await writeWantedLockfile(projectDir, dedicatedLockfile)
}

function pickProjectImporters (
  allImporters: LockfileObject['importers'],
  baseImporterId: string
): LockfileObject['importers'] {
  const importers: LockfileObject['importers'] = {}
  for (const [importerId, importer] of Object.entries(allImporters)) {
    if (importerId.startsWith(`${baseImporterId}/`)) {
      const newImporterId = importerId.slice(baseImporterId.length + 1) as ProjectId
      importers[newImporterId] = importer
      continue
    }
    if (importerId === baseImporterId) {
      importers['.' as ProjectId] = importer
    }
  }
  return importers
}

async function stageModulesDir (modulesDir: string, tempModulesDir: string): Promise<boolean> {
  try {
    await renameOverwrite(modulesDir, tempModulesDir)
    return true
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
    return false
  }
}

async function installFromDedicatedLockfile (projectDir: string): Promise<void> {
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
}

interface CollectedErrorsContext {
  projectDir: string
  tempModulesDir: string
  modulesRestored: boolean
}

function throwCollectedErrors (errors: unknown[], { projectDir, tempModulesDir, modulesRestored }: CollectedErrorsContext): void {
  if (errors.length === 1) {
    throw errors[0]
  }
  if (errors.length > 1) {
    const failures = errors.map((err) => isError(err) ? err.message : String(err))
    throw new PnpmError('MAKE_DEDICATED_LOCKFILE_FAILED', `Creating the dedicated lockfile in ${projectDir} failed:\n${failures.join('\n')}`, {
      cause: new AggregateError(errors, undefined, { cause: errors[0] }),
      hint: modulesRestored ? undefined : `The original node_modules is still in ${tempModulesDir}.`,
    })
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
    const publishedDeps = result[depField]
    if (deps == null || publishedDeps == null) continue
    const workspaceDeps = pickWorkspaceSpecs(deps, depField === 'peerDependencies')
    if (workspaceDeps != null) {
      result[depField] = { ...publishedDeps, ...workspaceDeps }
    }
  }
  return result
}

function pickWorkspaceSpecs (deps: Record<string, string>, isPeerField: boolean): Record<string, string> | undefined {
  let workspaceDeps: Record<string, string> | undefined
  for (const [depName, spec] of Object.entries(deps)) {
    const isWorkspace = isPeerField ? spec.includes('workspace:') : spec.startsWith('workspace:')
    if (!isWorkspace) continue
    workspaceDeps ??= {}
    workspaceDeps[depName] = spec
  }
  return workspaceDeps
}
