import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  registriesByScope: { default: 'https://registry.npmjs.org' },
  virtualStoreDir: 'node_modules/.pnpm',
}
