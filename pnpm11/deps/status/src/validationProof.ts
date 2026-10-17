import path from 'node:path'

import type { ProjectManifest } from '@pnpm/types'
import {
  fileSignatureTrusted,
  importerIdOf,
  lockfileDigest,
  manifestDigest,
  projectValidation,
  type Validation,
  type WorkspaceState,
} from '@pnpm/workspace.state'

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

/**
 * What the validation `workspaceState` records still proves, or `undefined`
 * when it cannot tell, which leaves the check to the modification times: the
 * state records no validation, or the wanted lockfile is missing and the
 * current one stands in for it.
 */
export function validationProof (
  workspaceState: WorkspaceState,
  lockfileDir: string,
  wantedLockfileName: string
): ValidationProof | undefined {
  const validation = workspaceState.validation
  if (validation == null) return undefined
  const lockfile = lockfileDigest(validation, lockfileDir, wantedLockfileName)
  if (lockfile.path !== path.join(lockfileDir, wantedLockfileName)) return undefined
  const digest = lockfile.digest.kind === 'present' ? lockfile.digest.signature.digest : undefined
  const lockfileChanged = digest !== validation.files?.[lockfile.path]?.digest
  return {
    lockfileChanged,
    lockfileUnreadable: lockfile.digest.kind === 'unreadable',
    rehashed: digest != null && !lockfileChanged && !fileSignatureTrusted(validation, lockfile.path),
    proofOf: (rootDir, manifest) => proofOf(validation, importerIdOf(lockfileDir, rootDir), manifest, digest),
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
