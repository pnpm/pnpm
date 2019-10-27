import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import path = require('path')

export default function testDefaults (opts?: any): any & { storeDir: string } { // tslint:disable-line
  return Object.assign({
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    storeDir: path.resolve('..', '.store'),
  }, opts)
}
