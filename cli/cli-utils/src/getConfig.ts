import fs from 'node:fs'
import path from 'node:path'

import { packageManager } from '@pnpm/cli-meta'
import { type CliOptions, type Config, getConfig as _getConfig } from '@pnpm/config'
import { resolveAndInstallConfigDeps } from '@pnpm/config.deps-installer'
import { formatWarn } from '@pnpm/default-reporter'
import { requireHooks } from '@pnpm/pnpmfile'
import { createStoreController } from '@pnpm/store-connection-manager'
import type { ConfigDependencies } from '@pnpm/types'
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
  applyDerivedConfig(config)

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/core expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.warn(warnings.map((warning) => formatWarn(warning)).join('\n'))
  }

  return config
}

export async function installConfigDepsAndLoadHooks (config: Config): Promise<Config> {
  if (config.configDependencies) {
    const store = await createStoreController(config)
    await resolveAndInstallConfigDeps(config.configDependencies, {
      ...config,
      store: store.ctrl,
      storeDir: store.dir,
      rootDir: config.lockfileDir ?? config.rootProjectManifestDir,
    })
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
  return config
}

export function * calcPnpmfilePathsOfPluginDeps (configModulesDir: string, configDependencies: ConfigDependencies): Generator<string> {
  for (const configDepName of Object.keys(configDependencies).sort(lexCompare)) {
    if (isPluginName(configDepName)) {
      const mjsPath = path.join(configModulesDir, configDepName, 'pnpmfile.mjs')
      if (fs.existsSync(mjsPath)) {
        yield mjsPath
      } else {
        yield path.join(configModulesDir, configDepName, 'pnpmfile.cjs')
      }
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
