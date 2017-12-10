import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import {read, save, Store} from './fs/storeIndex'
import resolveStore from './resolveStore'
import createStore, {StoreController} from './storeController'

export default createStore

export {
  Store,
  read,
  save,
  getRegistryName,
  resolveStore,
  StoreController,
}
