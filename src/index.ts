import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import {read, save} from './fs/storeIndex'
import resolveStore from './resolveStore'
import createStore, {StoreController} from './storeController'

export default createStore

export {
  read,
  save,
  getRegistryName,
  resolveStore,
  StoreController,
}
