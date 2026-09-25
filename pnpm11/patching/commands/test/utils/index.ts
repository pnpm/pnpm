import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  peersSuffixMaxLength: 1000,
  supportedArchitectures: {
    os: ['current'],
    cpu: ['current'],
    libc: ['current'],
  },
}
