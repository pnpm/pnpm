import {PnpmOptions} from '../../src'
import path = require('path')

export default function testDefaults (opts?: PnpmOptions): PnpmOptions & {globalPath: string, storePath: string} {
  return Object.assign({
    storePath: path.join(process.cwd(), '..', '.store'),
    cachePath: path.join(process.cwd(), '..', '.cache'),
    registry: 'http://localhost:4873/',
    globalPath: path.join(process.cwd(), '..', 'global'),
  }, opts)
}
