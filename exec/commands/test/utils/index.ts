import path from 'node:path'

import { tempDir } from '@pnpm/prepare'
import { DEFAULT_OPTS as BASE_OPTS, REGISTRY_URL } from '@pnpm/testing.command-defaults'

export { REGISTRY_URL }

const tmp = tempDir()

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
  extraBinPaths: [],
  supportedArchitectures: {
    os: ['current'],
    cpu: ['current'],
    libc: ['current'],
  },
}

export const DLX_DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
  cacheDir: path.join(tmp, 'cache'),
  dlxCacheMaxAge: Infinity,
  extraBinPaths: [],
  lock: true,
  pnpmfile: ['.pnpmfile.cjs'],
  storeDir: path.join(tmp, 'store'),
  symlink: true,
  supportedArchitectures: {
    os: ['current'],
    cpu: ['current'],
    libc: ['current'],
  },
  workspaceConcurrency: 1,
}
