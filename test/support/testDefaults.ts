import {PnpmOptions} from '../../src'
import globalPath from './globalPath'

export default function testDefaults (opts?: PnpmOptions): PnpmOptions {
  return Object.assign({
    storePath: 'node_modules/.store',
    registry: 'http://localhost:4873/',
    globalPath,
  }, opts)
}
