import {Shrinkwrap} from 'pnpm-shrinkwrap'
import R = require('ramda')

export default function shrinkwrapsEqual (shr1: Shrinkwrap, shr2: Shrinkwrap) {
  const specs1 = R.keys(shr1.specifiers)
  const specs2 = R.keys(shr2.specifiers)
  if (specs1.length !== specs2.length || !R.equals(specs1, specs2)) {
    return false
  }
  const pkgs1 = R.keys(shr1.packages)
  const pkgs2 = R.keys(shr2.packages)
  return pkgs1.length === pkgs2.length && R.equals(pkgs1, pkgs2)
}
