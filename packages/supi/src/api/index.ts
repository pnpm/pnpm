export * from './install'
export * from './link'
export * from './prune'
export * from './unlink'
export * from './rebuild'

import link from './link'
import storePrune from './storePrune'
import storeStatus from './storeStatus'
import uninstall from './uninstall'
export {
  link,
  uninstall,
  storeStatus,
  storePrune,
}
