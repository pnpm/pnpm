import path from 'path'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

export default function testDefaults (opts?: any): any & { storeDir: string } { // eslint-disable-line
  return {
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    storeDir: path.resolve('..', '.store'),
    ...opts,
  }
}
