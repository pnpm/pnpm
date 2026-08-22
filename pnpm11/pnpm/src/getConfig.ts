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
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { ConfigDependencies } from '@pnpm/types'

export async function getConfig (
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean
    workspaceDir: string | undefined
    onlyInheritDlxSettingsFromLocal?: boolean
    forSelfUpdate?: boolean
    printWarnings?: boolean
  }
): Promise<{ config: Config, context: ConfigContext }> {
  const { config, context, warnings } = await _getConfig({
    cliOptions,
    globalDirShouldAllowWrite: opts.globalDirShouldAllowWrite,
    packageManager,
    workspaceDir: opts.workspaceDir,
    onlyInheritDlxSettingsFromLocal: opts.onlyInheritDlxSettingsFromLocal,
    forSelfUpdate: opts.forSelfUpdate,
  })
  context.cliOptions = cliOptions
  applyDerivedConfig(config)

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/installing.deps-installer expects a function as opts.reporter
  }

  if (opts.printWarnings !== false && warnings.length > 0) {
    console.warn(warnings.map((warning) => formatWarn(warning)).join('\n'))
  }

  return { config, context }
}

/**
 * Whether the invocation prints one setting's value (`pnpm config get <key>`
 * or `pnpm get <key>`). Such reads are consumed by scripts, so config-load
 * warnings stay off them; the keyless list forms keep the warnings, being how
 * a user inspects the config.
 */
export function isSingleSettingRead (cmd: string | null, cliParams: string[]): boolean {
  if (cmd === 'config') return cliParams[0] === 'get' && cliParams.length > 1
  return cmd === 'get' && cliParams.length > 0
}

export async function installConfigDepsAndLoadHooks (
  config: Config,
  context: ConfigContext,
  opts?: {
    tolerateConfigDependenciesErrors?: boolean
    // Set by `self-update`: don't auto-load the repo-controlled default
    // `.pnpmfile.(c|m)js`. Its `updateConfig` hook could rewrite any setting —
    // including the release-age policy the config reader just resolved for
    // self-update — and its `customResolvers`/`customFetchers` would take over
    // the pnpm download the trusted bootstrap registry is there to protect.
    // Pnpmfiles from trusted sources (the `pnpmfile` setting, the global
    // pnpmfile, config-dependency plugins) are still loaded.
    forSelfUpdate?: boolean
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
    config.tryLoadDefaultPnpmfile = config.pnpmfile == null && !opts?.forSelfUpdate
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
      reapplyExplicitCliOptions(config, context)
    }
  }
  return { config, context }
}

function reapplyExplicitCliOptions (config: Config, context: ConfigContext): void {
  const cliOptions = context.cliOptions
  if (!cliOptions) return
  if (cliOptions.registry && typeof cliOptions.registry === 'string') {
    const normalized = normalizeRegistryUrl(cliOptions.registry)
    config.registry = normalized
    if (config.registriesByScope) {
      config.registriesByScope.default = normalized
    }
    if (config.packageManagerRegistries) {
      config.packageManagerRegistries.default = normalized
    }
    if ((config as Record<string, any>).registries?.default) { // eslint-disable-line @typescript-eslint/no-explicit-any
      (config as Record<string, any>).registries.default = normalized // eslint-disable-line @typescript-eslint/no-explicit-any
    }
  }
  for (const [key, value] of Object.entries(cliOptions)) {
    if (key.startsWith('@') && key.endsWith(':registry') && typeof value === 'string') {
      const scope = key.slice(0, -':registry'.length)
      const normalized = normalizeRegistryUrl(value)
      if (config.registriesByScope) {
        config.registriesByScope[scope] = normalized
      }
      if (config.packageManagerRegistries) {
        config.packageManagerRegistries[scope] = normalized
      }
      if ((config as Record<string, any>).registries) { // eslint-disable-line @typescript-eslint/no-explicit-any
        (config as Record<string, any>).registries[scope] = normalized // eslint-disable-line @typescript-eslint/no-explicit-any
      }
    }
  }
  if (context.explicitlySetKeys) {
    for (const key of context.explicitlySetKeys) {
      if (key === 'registry') continue
      const camelKey = key.replace(/-([a-z])/g, (_, g: string) => g.toUpperCase())
      const kebabKey = key.replace(/([A-Z])/g, '-$1').toLowerCase()
      const val = cliOptions[key] ?? cliOptions[camelKey] ?? cliOptions[kebabKey]
      if (val !== undefined) {
        (config as unknown as Record<string, unknown>)[key] = val
        if (key !== camelKey) {
          (config as unknown as Record<string, unknown>)[camelKey] = val
        }
      }
    }
  }
}

function normalizeRegistryUrl (url: string): string {
  return url.endsWith('/') ? url : `${url}/`
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
