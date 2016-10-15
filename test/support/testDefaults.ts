import {PnpmOptions} from '../../src'

export default function testDefaults (opts?: PnpmOptions): PnpmOptions {
  return Object.assign({
    storePath: 'node_modules/.store',
    registry: 'http://localhost:4873/',
  }, opts)
}
