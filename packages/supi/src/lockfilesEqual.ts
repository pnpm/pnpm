import { Lockfile } from '@pnpm/lockfile-file'
import R = require('ramda')

export default function lockfilesEqual (lockfile1: Lockfile, lockfile2: Lockfile) {
  const importers1 = R.keys(lockfile1.importers)
  const importers2 = R.keys(lockfile2.importers)
  if (importers1.length !== importers2.length || !R.equals(importers1, importers2)) {
    return false
  }
  const pkgs1 = R.keys(lockfile1.packages)
  const pkgs2 = R.keys(lockfile2.packages)
  return pkgs1.length === pkgs2.length && R.equals(pkgs1, pkgs2)
}
