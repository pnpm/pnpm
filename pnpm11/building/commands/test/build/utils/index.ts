import { DEFAULT_OPTS as BASE_OPTS, REGISTRY_URL } from '@pnpm/testing.command-defaults'

export { REGISTRY_URL as REGISTRY }

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
  deployAllFiles: false,
  supportedArchitectures: {
    os: ['current'],
    cpu: ['current'],
    libc: ['current'],
  },
}
