import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { formatWarn } from '@pnpm/cli.default-reporter'
import { packageManager } from '@pnpm/cli.meta'
import { type CliOptions, type Config, type ConfigContext, getConfig as _getConfig } from '@pnpm/config.reader'
import { requireHooks } from '@pnpm/hooks.pnpmfile'
import { resolveAndInstallConfigDeps } from '@pnpm/installing.env-installer'
import { logger } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store.connection-manager'
import type { ConfigDependencies } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'

export async function getConfig (
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean
    workspaceDir: string | undefined
    onlyInheritDlxSettingsFromLocal?: boolean
  }
): Promise<{ config: Config, context: ConfigContext }> {
  const { config, context, warnings } = await _getConfig({
    cliOptions,
    globalDirShouldAllowWrite: opts.globalDirShouldAllowWrite,
    packageManager,
    workspaceDir: opts.workspaceDir,
    onlyInheritDlxSettingsFromLocal: opts.onlyInheritDlxSettingsFromLocal,
  })
  context.cliOptions = cliOptions
  applyDerivedConfig(config)

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/installing.deps-installer expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.warn(warnings.map((warning) => formatWarn(warning)).join('\n'))
  }

  return { config, context }
}

export async function installConfigDepsAndLoadHooks (
  config: Config,
  context: ConfigContext,
  opts?: {
    tolerateConfigDependenciesErrors?: boolean
  }
): Promise<{ config: Config, context: ConfigContext }> {
  if (config.configDependencies) {
    const store = await createStoreController({ ...config, ...context })
    try {
      await resolveAndInstallConfigDeps(config.configDependencies, {
        ...config,
        ...context,
        store: store.ctrl,
        storeDir: store.dir,
        rootDir: config.lockfileDir ?? context.rootProjectManifestDir,
        frozenLockfile: config.frozenLockfile,
      })
    } catch (err: unknown) {
      if (!opts?.tolerateConfigDependenciesErrors) {
        throw err
      }
      const errorMessage = util.types.isNativeError(err) ? err.message : String(err)
      logger.debug({
        message: `Failed to install configDependencies. This is expected if authentication is not yet configured. Proceeding. Error: ${errorMessage}`,
        err,
      })
    } finally {
      await store.ctrl.close()
    }
  }
  if (!config.ignorePnpmfile) {
    config.tryLoadDefaultPnpmfile = config.pnpmfile == null
    const pnpmfiles = config.pnpmfile == null ? [] : Array.isArray(config.pnpmfile) ? config.pnpmfile : [config.pnpmfile]
    if (config.configDependencies) {
      const configModulesDir = path.join(config.lockfileDir ?? context.rootProjectManifestDir, 'node_modules/.pnpm-config')
      pnpmfiles.unshift(...calcPnpmfilePathsOfPluginDeps(configModulesDir, config.configDependencies))
    }
    const { hooks, finders, resolvedPnpmfilePaths } = await requireHooks(config.lockfileDir ?? config.dir, {
      globalPnpmfile: config.globalPnpmfile,
      pnpmfiles,
      tryLoadDefaultPnpmfile: config.tryLoadDefaultPnpmfile,
    })
    context.hooks = hooks
    context.finders = finders
    config.pnpmfile = resolvedPnpmfilePaths
    if (context.hooks?.updateConfig) {
      for (const updateConfig of context.hooks.updateConfig) {
        const updateConfigResult = updateConfig(config)
        config = updateConfigResult instanceof Promise ? await updateConfigResult : updateConfigResult // eslint-disable-line no-await-in-loop
      }
    }
  }
  return { config, context }
}

export function * calcPnpmfilePathsOfPluginDeps (configModulesDir: string, configDependencies: ConfigDependencies): Generator<string> {
  for (const configDepName of Object.keys(configDependencies).sort(lexCompare)) {
    if (isPluginName(configDepName)) {
      const pluginDir = path.join(configModulesDir, configDepName)
      // If the plugin directory itself is missing, the install didn't run
      // (or hasn't run yet) — skip silently. If the plugin directory exists
      // but contains no pnpmfile, fall through to yield the .cjs path so
      // requireHooks surfaces PNPMFILE_NOT_FOUND for the misconfigured plugin.
      if (!fs.existsSync(pluginDir)) continue
      const mjsPath = path.join(pluginDir, 'pnpmfile.mjs')
      if (fs.existsSync(mjsPath)) {
        yield mjsPath
        continue
      }
      yield path.join(pluginDir, 'pnpmfile.cjs')
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
