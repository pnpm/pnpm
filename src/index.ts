import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import pkgIdToFilename from './fs/pkgIdToFilename'
import {read, save, Store} from './fs/storeIndex'
import pkgIsUntouched from './pkgIsUntouched'
import resolveStore from './resolveStore'
import createStore, {StoreController} from './storeController'

export default createStore

export {
  pkgIdToFilename,
  Store,
  read,
  save,
  getRegistryName,
  pkgIsUntouched,
  resolveStore,
  StoreController,
}
