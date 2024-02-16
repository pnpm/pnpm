import { packageManager } from '@pnpm/cli-meta'
import { getConfig as _getConfig, type CliOptions } from '@pnpm/config'
import { formatWarn } from '@pnpm/default-reporter'

export async function getConfig (
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean
    rcOptionsTypes: Record<string, unknown>
    workspaceDir: string | undefined
    checkUnknownSetting?: boolean
  }
) {
  const { config, warnings } = await _getConfig({
    cliOptions,
    globalDirShouldAllowWrite: opts.globalDirShouldAllowWrite,
    packageManager,
    rcOptionsTypes: opts.rcOptionsTypes,
    workspaceDir: opts.workspaceDir,
    checkUnknownSetting: opts.checkUnknownSetting,
  })
  config.cliOptions = cliOptions

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/core expects a function as opts.reporter
  }

  // The warning should not be printed when --json is specified
  if (warnings.length > 0 && !cliOptions.json) {
    console.log(warnings.map((warning) => formatWarn(warning)).join('\n'))
  }

  return config
}
