import { PnpmError } from '@pnpm/error'

export class LockfileConfigMismatchError extends PnpmError {
  constructor (outdatedLockfileSettingName: string) {
    super('LOCKFILE_CONFIG_MISMATCH',
      `Cannot proceed with the frozen installation. The current "${outdatedLockfileSettingName!}" configuration doesn't match the value found in the lockfile`, {
        hint: 'Update your lockfile using "pnpm install --no-frozen-lockfile"',
      })
  }
}

export class InconsistentPatchHashError extends PnpmError {
  constructor () {
    super('INCONSISTENT_PATCH_HASH',
      'Cannot proceed with the frozen installation. The lockfile records dependency paths whose ' +
      'patch hashes disagree with its own "patchedDependencies"', {
        hint: 'The lockfile disagrees with itself, which usually means it was hand-edited or a merge conflict was incorrectly resolved. ' +
          'Repair your lockfile using "pnpm install --no-frozen-lockfile"',
      })
  }
}

export class UncheckablePatchHashError extends PnpmError {
  constructor () {
    super('UNCHECKABLE_PATCH_HASH',
      'Cannot proceed with the frozen installation. The lockfile\'s patch hashes cannot be checked ' +
      'against its own "patchedDependencies"', {
        hint: 'The lockfile has a malformed patch hash, or is missing a package version or a usable "patchedDependencies" entry that checking needs. ' +
          'Repair your lockfile using "pnpm install --no-frozen-lockfile"',
      })
  }
}

export const BROKEN_LOCKFILE_INTEGRITY_ERRORS = new Set([
  'ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE',
  'ERR_PNPM_TARBALL_INTEGRITY',
])
