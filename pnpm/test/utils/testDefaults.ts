import path from 'node:path'

import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function testDefaults(opts?: any): any & { storeDir: string } {
  return {
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    storeDir: path.resolve('..', '.store'),
    ...opts,
  }
}
