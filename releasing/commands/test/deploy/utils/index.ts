import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  deployAllFiles: false,
  injectWorkspacePackages: true,
}
