export * from './install'
export * from './link'
export * from './prune'
export * from './unlink'
export * from './rebuild'

import link from './link'
import storeStatus from './storeStatus'
import storePrune from './storePrune'
import uninstall from './uninstall'
export {
  link,
  uninstall,
  storeStatus,
  storePrune,
}
