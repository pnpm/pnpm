import path from 'node:path'

import type { Project, ProjectManifest } from '@pnpm/types'
import {
  fileDigestOf,
  fileSignatureTrusted,
  importerIdOf,
  lockfileDigest,
  manifestDigest,
  projectValidation,
  sharedInputPaths,
  updateWorkspaceState,
  type Validation,
  type WorkspaceState,
} from '@pnpm/workspace.state'

import type { CheckDepsStatusOptions } from './types.js'

/** Whether a project is still proven installed. */
export type ProjectProof =
  | 'proven'
  /** The manifest is not the one the project was proven installed with. */
  | 'manifestChanged'
  /**
   * The lockfile is not the one the project was proven installed with, and
   * the manifest may not be either.
   */
  | 'lockfileChanged'
  /** No validation records the project as installed. */
  | 'unrecorded'

export interface ValidationProof {
  /** Whether the lockfile's content differs from what the validation recorded. */
  lockfileChanged: boolean
  /** Whether the lockfile could not be read, so nothing about it is proven. */
  lockfileUnreadable: boolean
  /**
   * Whether the lockfile's recorded stat no longer stood for its content, which
   * was read again and found unchanged.
   */
  rehashed: boolean
  proofOf: (rootDir: string, manifest: ProjectManifest) => ProjectProof
}

export interface ValidationProofOptions {
  wantedLockfileName: string
  unmergedBranchLockfiles: boolean
  sharedWorkspaceLockfile?: boolean
  hasUnmergedBranchLockfiles?: (dir: string) => boolean
}

/**
 * What the validation `workspaceState` records still proves, or `undefined`
 * when it cannot tell, which leaves the check to the modification times: the
 * state records no validation, or the wanted lockfile is missing and the
 * current one stands in for it.
 */
export function validationProof (
  workspaceState: WorkspaceState,
  lockfileDir: string,
  opts: ValidationProofOptions
): ValidationProof | undefined {
  const validation = workspaceState.validation
  if (validation == null) return undefined
  if (opts.sharedWorkspaceLockfile === false) {
    return perProjectValidationProof(validation, lockfileDir, opts)
  }
  const lockfile = lockfileDigest(validation, lockfileDir, opts.wantedLockfileName)
  if (lockfile.path !== path.join(lockfileDir, opts.wantedLockfileName)) return undefined
  // The recorded digest covers the main lockfile only, so a branch lockfile
  // still to be merged changes the wanted lockfile without changing it.
  const digest = lockfile.digest.kind === 'present' && !opts.unmergedBranchLockfiles ? lockfile.digest.signature.digest : undefined
  const lockfileChanged = digest !== validation.files?.[lockfile.path]?.digest
  return {
    lockfileChanged,
    lockfileUnreadable: lockfile.digest.kind === 'unreadable',
    rehashed: digest != null && !lockfileChanged && !fileSignatureTrusted(validation, lockfile.path),
    proofOf: (rootDir, manifest) => proofOf(validation, importerIdOf(lockfileDir, rootDir), manifest, digest),
  }
}

interface PerProjectLockfileCheck {
  digest: string | undefined
  changed: boolean
  unreadable: boolean
  rehashed: boolean
}

function perProjectValidationProof (
  validation: Validation,
  lockfileDir: string,
  opts: ValidationProofOptions
): ValidationProof {
  const byDir = new Map<string, PerProjectLockfileCheck>()
  return {
    get lockfileChanged () {
      return [...byDir.values()].some((entry) => entry.changed)
    },
    get lockfileUnreadable () {
      return [...byDir.values()].some((entry) => entry.unreadable)
    },
    get rehashed () {
      return [...byDir.values()].some((entry) => entry.rehashed)
    },
    proofOf: (rootDir, manifest) => {
      const importerId = importerIdOf(lockfileDir, rootDir)
      const record = projectValidation(validation, importerId)
      if (record == null) return 'unrecorded'
      let check = byDir.get(rootDir)
      if (check == null) {
        check = checkProjectLockfile(validation, rootDir, opts)
        byDir.set(rootDir, check)
      }
      if (check.unreadable || record.lockfile !== check.digest) return 'lockfileChanged'
      if (record.manifest !== manifestDigest(manifest)) return 'manifestChanged'
      return 'proven'
    },
  }
}

function checkProjectLockfile (
  validation: Validation,
  rootDir: string,
  { wantedLockfileName, unmergedBranchLockfiles, hasUnmergedBranchLockfiles }: ValidationProofOptions
): PerProjectLockfileCheck {
  const wantedPath = path.join(rootDir, wantedLockfileName)
  const lockfile = fileDigestOf(validation, wantedPath)
  const hasBranch = hasUnmergedBranchLockfiles?.(rootDir) ?? unmergedBranchLockfiles
  const digest = lockfile.kind === 'present' && !hasBranch
    ? lockfile.signature.digest
    : undefined
  const changed = digest !== validation.files?.[wantedPath]?.digest
  return {
    digest,
    changed,
    unreadable: lockfile.kind === 'unreadable',
    rehashed: digest != null && !changed && !fileSignatureTrusted(validation, wantedPath),
  }
}

function proofOf (validation: Validation, importerId: string, manifest: ProjectManifest, lockfile: string | undefined): ProjectProof {
  const record = projectValidation(validation, importerId)
  if (record == null) return 'unrecorded'
  // A changed lockfile calls for every check a changed manifest does, and more.
  if (record.lockfile !== lockfile) return 'lockfileChanged'
  if (record.manifest !== manifestDigest(manifest)) return 'manifestChanged'
  return 'proven'
}

export interface RecordValidatedWorkspaceStateOptions {
  allProjects: Array<Pick<Project, 'rootDir' | 'manifest'>>
  lockfileDir: string
  opts: CheckDepsStatusOptions
  workspaceState: WorkspaceState
  wantedLockfileName: string
  vouchedProjects?: string[]
  onlyIfTrusted?: boolean
}

export async function recordValidatedWorkspaceState (
  { allProjects, lockfileDir, onlyIfTrusted, opts, vouchedProjects, wantedLockfileName, workspaceState }: RecordValidatedWorkspaceStateOptions
): Promise<void> {
  await updateWorkspaceState({
    allProjects,
    workspaceDir: lockfileDir,
    pnpmfiles: workspaceState.pnpmfiles,
    settings: {
      ...opts,
      autoDedupe: opts.autoDedupe ?? workspaceState.settings.autoDedupe,
    },
    ignoredWorkspaceStateSettings: opts.ignoredWorkspaceStateSettings,
    filteredInstall: workspaceState.filteredInstall,
    configDependencies: workspaceState.configDependencies,
    validation: {
      lockfileDir,
      wantedLockfileName,
      sharedInputs: sharedInputPaths(lockfileDir, opts.patchedDependencies, opts.pnpmfile),
      vouchedProjects,
      onlyIfTrusted,
      sharedWorkspaceLockfile: opts.sharedWorkspaceLockfile,
    },
  })
}
