import { read, save } from './fs/storeIndex'
import createStore from './storeController'

export default createStore

export {
  read,
  save,
}

export * from '@pnpm/store-controller-types'
