import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { equals } from 'ramda'

export interface AssertLockfilesEqualOptions {
  wantedLockfileDir: string
  /**
   * The current lockfile was written by a filtered install, which records
   * only the packages of the projects it selected.
   */
  filteredInstall?: boolean
}

export function assertLockfilesEqual (currentLockfile: LockfileObject | null, wantedLockfile: LockfileObject, opts: AssertLockfilesEqualOptions): void {
  if (!currentLockfile) {
    // make sure that no importer of wantedLockfile has any dependency
    for (const [name, snapshot] of Object.entries(wantedLockfile.importers)) {
      if (!equals(snapshot.specifiers, {})) {
        throw new PnpmError('RUN_CHECK_DEPS_NO_DEPS', `Project ${name} requires dependencies but none was installed.`, {
          hint: 'Run `pnpm install` to install dependencies',
        })
      }
    }
  } else if (
    !equals(currentLockfile, wantedLockfile) &&
    !(opts.filteredInstall === true && recordsSubsetOfPackages(currentLockfile, wantedLockfile))
  ) {
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_DEPS', `The installed dependencies in the modules directory is not up-to-date with the lockfile in ${opts.wantedLockfileDir}.`, {
      hint: 'Run `pnpm install` to update dependencies.',
    })
  }
}

/**
 * Whether `currentLockfile` equals `wantedLockfile` except for the packages
 * it leaves out. With the importers and every recorded package unchanged,
 * the packages any installed project reaches are unchanged too.
 */
function recordsSubsetOfPackages (currentLockfile: LockfileObject, wantedLockfile: LockfileObject): boolean {
  const { packages: currentPackages, ...currentRest } = currentLockfile
  const { packages: wantedPackages, ...wantedRest } = wantedLockfile
  return equals(currentRest, wantedRest) &&
    Object.entries(currentPackages ?? {}).every(([depPath, pkg]) =>
      equals(pkg, wantedPackages?.[depPath as keyof typeof wantedPackages]))
}
