import { Lockfile } from '@pnpm/lockfile-file'
import R = require('ramda')

export default function (
  currentLockfile: Lockfile,
  opts: {
    skippedPkgIds: string[],
    wantedLockfile: Lockfile,
  }
) {
  const importers1 = R.keys(opts.wantedLockfile.importers)
  const importers2 = R.keys(currentLockfile.importers)
  if (importers1.length !== importers2.length || !R.equals(importers1, importers2)) {
    return false
  }
  const pkgs1 = R.keys(opts.wantedLockfile.packages)
  const pkgs2 = R.keys(currentLockfile.packages).concat(opts.skippedPkgIds)
  return pkgs1.length === pkgs2.length && R.equals(pkgs1, pkgs2.sort())
}
