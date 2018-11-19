import { Shrinkwrap } from '@pnpm/shrinkwrap-file'
import R = require('ramda')

export default function shrinkwrapsEqual (shr1: Shrinkwrap, shr2: Shrinkwrap) {
  const importers1 = R.keys(shr1.importers)
  const importers2 = R.keys(shr2.importers)
  if (importers1.length !== importers2.length || !R.equals(importers1, importers2)) {
    return false
  }
  const pkgs1 = R.keys(shr1.packages)
  const pkgs2 = R.keys(shr2.packages)
  return pkgs1.length === pkgs2.length && R.equals(pkgs1, pkgs2)
}
