import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import pkgIdToFilename from './fs/pkgIdToFilename'
import {read, save, Store} from './fs/storeController'
import pkgIsUntouched from './pkgIsUntouched'
import resolveStore from './resolveStore'

export {
  pkgIdToFilename,
  Store,
  read,
  save,
  getRegistryName,
  pkgIsUntouched,
  resolveStore,
}
