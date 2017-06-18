import {PnpmOptions} from '../../src'
import path = require('path')

export default function testDefaults (opts?: PnpmOptions): PnpmOptions  & {store: string} {
  return Object.assign({
    store: path.resolve('..', '.store'),
    registry: 'http://localhost:4873/',
  }, opts)
}
