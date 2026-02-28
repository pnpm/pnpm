import path from 'path'
import { packageManager } from '@pnpm/cli-meta'
import { getConfig as _getConfig, type CliOptions, type Config } from '@pnpm/config'
import { formatWarn } from '@pnpm/default-reporter'
import { createStoreController } from '@pnpm/store-connection-manager'
import { installConfigDeps } from '@pnpm/config.deps-installer'
import { requireHooks } from '@pnpm/pnpmfile'
import { type ConfigDependencies } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'

export async function getConfig (
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean
    rcOptionsTypes: Record<string, unknown>
    workspaceDir: string | undefined
    checkUnknownSetting?: boolean
    ignoreNonAuthSettingsFromLocal?: boolean
    catchConfigDependenciesErrors?: boolean
  }
): Promise<Config> {
  let { config, warnings } = await _getConfig({
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
    try {
      const store = await createStoreController(config)
      try {
        await installConfigDeps(config.configDependencies, {
          registries: config.registries,
          rootDir: config.lockfileDir ?? config.rootProjectManifestDir,
          store: store.ctrl,
        })
      } finally {
        if (typeof store?.ctrl?.close === 'function') {
          try {
            await store.ctrl.close()
          } catch (cleanupErr) {
            console.debug('Failed to close store controller:', cleanupErr)
          }
        }
      }
    } catch (err: unknown) {
      if (opts.catchConfigDependenciesErrors) {
        try {
          const { logger } = await import('@pnpm/logger')
          const errorMessage = err instanceof Error ? err.message : String(err)

          logger.debug({
            message: `Failed to install configDependencies. This is expected if authentication is not yet configured. Proceeding. Error: ${errorMessage}`,
            err,
          })
        } catch {
          //ignore error
        }
      } else {
        throw err
      }
    }
  }
  if (!config.ignorePnpmfile) {
    config.tryLoadDefaultPnpmfile = config.pnpmfile == null
    const pnpmfiles = config.pnpmfile == null ? [] : Array.isArray(config.pnpmfile) ? config.pnpmfile : [config.pnpmfile]
    if (config.configDependencies) {
      const configModulesDir = path.join(config.lockfileDir ?? config.rootProjectManifestDir, 'node_modules/.pnpm-config')
      pnpmfiles.unshift(...calcPnpmfilePathsOfPluginDeps(configModulesDir, config.configDependencies))
    }
    const { hooks, finders, resolvedPnpmfilePaths } = await requireHooks(config.lockfileDir ?? config.dir, {
      globalPnpmfile: config.globalPnpmfile,
      pnpmfiles,
      tryLoadDefaultPnpmfile: config.tryLoadDefaultPnpmfile,
    })
    config.hooks = hooks
    config.finders = finders
    config.pnpmfile = resolvedPnpmfilePaths
    if (config.hooks?.updateConfig) {
      for (const updateConfig of config.hooks.updateConfig) {
        const updateConfigResult = updateConfig(config)
        config = updateConfigResult instanceof Promise ? await updateConfigResult : updateConfigResult // eslint-disable-line no-await-in-loop
      }
    }
  }
  applyDerivedConfig(config)

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/core expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.warn(warnings.map((warning) => formatWarn(warning)).join('\n'))
  }

  return config
}

function * calcPnpmfilePathsOfPluginDeps (configModulesDir: string, configDependencies: ConfigDependencies): Generator<string> {
  for (const configDepName of Object.keys(configDependencies).sort(lexCompare)) {
    if (isPluginName(configDepName)) {
      yield path.join(configModulesDir, configDepName, 'pnpmfile.cjs')
    }
  }
}

function isPluginName (configDepName: string): boolean {
  if (configDepName.startsWith('pnpm-plugin-')) return true
  if (configDepName[0] !== '@') return false
  return configDepName.startsWith('@pnpm/plugin-') || configDepName.includes('/pnpm-plugin-')
}

// Apply derived config settings (hoist, shamefullyHoist, symlink)
function applyDerivedConfig (config: Config): void {
  if (config.hoist === false) {
    delete config.hoistPattern
  }
  switch (config.shamefullyHoist) {
  case false:
    delete config.publicHoistPattern
    break
  case true:
    config.publicHoistPattern = ['*']
    break
  default:
    if (
      (config.publicHoistPattern == null) ||
        (config.publicHoistPattern === '') ||
        (
          Array.isArray(config.publicHoistPattern) &&
          config.publicHoistPattern.length === 1 &&
          config.publicHoistPattern[0] === ''
        )
    ) {
      delete config.publicHoistPattern
    }
    break
  }
  if (!config.symlink) {
    delete config.hoistPattern
    delete config.publicHoistPattern
  }
}
