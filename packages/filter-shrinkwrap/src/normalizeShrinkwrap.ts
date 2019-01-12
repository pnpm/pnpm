import { Shrinkwrap } from '@pnpm/shrinkwrap-types'

export default function normalizeShrinkwrap (shr: Shrinkwrap) {
  if (!shr.registry) {
    delete shr.registry
  }
  return shr
}
