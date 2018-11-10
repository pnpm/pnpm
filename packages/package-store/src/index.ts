import { read, save } from './fs/storeIndex'
import { ImportingLog } from './loggers'
import createStore, { StoreController } from './storeController'

export default createStore

export {
  read,
  save,
  StoreController,
  ImportingLog,
}
