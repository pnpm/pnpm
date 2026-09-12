import type * as logs from '@pnpm/core-loggers'

/**
 * The slice of pnpm's resolved config the reporter reads. Structural on
 * purpose: the pnpm CLI passes its full `Config`, while other hosts (for
 * example Bit, which drives the engine through `@pnpm/napi`) can pass just
 * the fields they have without depending on `@pnpm/config.reader`.
 */
export interface ReporterPnpmConfig {
  dir?: string
  workspaceDir?: string
  global?: boolean
  recursive?: boolean
  production?: boolean
  dev?: boolean
  optional?: boolean
  saveDev?: boolean
  strictDepBuilds?: boolean
  authConfig?: Record<string, string>
  cliOptions?: Record<string, unknown>
  hooks?: {
    filterLog?: Array<(log: logs.Log) => boolean>
  }
}
