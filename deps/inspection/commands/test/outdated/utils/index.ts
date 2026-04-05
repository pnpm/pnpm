import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
  ci: false,
  deployAllFiles: false,
  global: false,
  tag: 'latest',
}

export const DEFAULT_OUTDATED_OPTS = {
  ...DEFAULT_OPTS,
  sortBy: 'name' as const,
}
