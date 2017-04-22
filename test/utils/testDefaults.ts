import {PnpmOptions} from '../../src'
import path = require('path')

export default function testDefaults (opts?: PnpmOptions): PnpmOptions  & {storePath: string} {
  return Object.assign({
    storePath: path.resolve('..', '.store'),
    localRegistry: path.resolve('..', '.registry'),
    registry: 'http://localhost:4873/',
  }, opts)
}
