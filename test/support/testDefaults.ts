import {PnpmOptions} from '../../src'
import path = require('path')

export default function testDefaults (opts?: PnpmOptions): PnpmOptions & {globalPath: string} {
  return Object.assign({
    storePath: path.join(process.cwd(), '..', '.store'),
    registry: 'http://localhost:4873/',
    globalPath: path.join(process.cwd(), '..', 'global'),
  }, opts)
}
