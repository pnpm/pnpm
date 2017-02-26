import {PnpmOptions} from '../../src'
import path = require('path')

export default function testDefaults (opts?: PnpmOptions): PnpmOptions & {globalPath: string, storePath: string} {
  return Object.assign({
    storePath: path.resolve('..', '.store'),
    localRegistry: path.resolve('..', '.registry'),
    cachePath: path.resolve('..', '.cache'),
    registry: 'http://localhost:4873/',
    globalPath: path.resolve('..', 'global'),
  }, opts)
}
