import path = require('path')
import { PnpmOptions } from 'supi'

export default function testDefaults (opts?: PnpmOptions): PnpmOptions & { store: string } {
  return Object.assign({
    registry: 'http://localhost:4873/',
    store: path.resolve('..', '.store'),
  }, opts)
}
