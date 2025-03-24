import { packageManager } from '@pnpm/cli-meta'
import { getConfig as _getConfig, type CliOptions, type Config } from '@pnpm/config'
import { formatWarn } from '@pnpm/default-reporter'
import { createOrConnectStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { installConfigDeps } from '@pnpm/config.deps-installer'
import { requireHooks } from '@pnpm/pnpmfile'

export async function getConfig (
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean
    rcOptionsTypes: Record<string, unknown>
    workspaceDir: string | undefined
    checkUnknownSetting?: boolean
    ignoreNonAuthSettingsFromLocal?: boolean
  }
): Promise<Config> {
  const { config, warnings } = await _getConfig({
    cliOptions,
    globalDirShouldAllowWrite: opts.globalDirShouldAllowWrite,
    packageManager,
    rcOptionsTypes: opts.rcOptionsTypes,
    workspaceDir: opts.workspaceDir,
    checkUnknownSetting: opts.checkUnknownSetting,
    ignoreNonAuthSettingsFromLocal: opts.ignoreNonAuthSettingsFromLocal,
  })
  config.cliOptions = cliOptions
  if (config.configDependencies) {
    let store = await createOrConnectStoreController(config)
    await installConfigDeps(opts.configDependencies, {
      regisoptstries: opts.registries,
      rootDir: opts.lockfileDir ?? opts.rootProjectManifestDir,
      store: store.ctrl,
    })
  }
  if (!opts.ignorePnpmfile && !opts.hooks) {
    opts.hooks = requireHooks(opts.lockfileDir ?? opts.dir, opts)
    if (opts.hooks.fetchers != null || opts.hooks.importPackage != null) {
      store = await createOrConnectStoreController(opts)
    }
  }

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/core expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.warn(warnings.map((warning) => formatWarn(warning)).join('\n'))
  }

  return config
}
