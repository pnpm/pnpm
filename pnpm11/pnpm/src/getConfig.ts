import fs from 'node:fs'
import path from 'node:path'

import { formatWarn } from '@pnpm/cli.default-reporter'
import { packageManager } from '@pnpm/cli.meta'
import { DEFAULT_REGISTRIES_BY_SCOPE, normalizeRegistriesByScope } from '@pnpm/config.normalize-registries'
import { type CliOptions, type Config, type ConfigContext, getConfig as _getConfig } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import { requireHooks } from '@pnpm/hooks.pnpmfile'
import { resolveAndInstallConfigDeps } from '@pnpm/installing.env-installer'
import { logger } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store.connection-manager'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { ConfigDependencies } from '@pnpm/types'
import camelcase from 'camelcase'
import { equals } from 'ramda'

export async function getConfig (
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean
    skipGlobalBinDirCheck?: boolean
    workspaceDir: string | undefined
    rawCliConfig?: Record<string, unknown>
    onlyInheritDlxSettingsFromLocal?: boolean
    forSelfUpdate?: boolean
    ignoreProjectNpmrc?: boolean
    printWarnings?: boolean
  }
): Promise<{ config: Config, context: ConfigContext }> {
  const warnings: string[] = []
  try {
    const { config, context } = await _getConfig({
      cliOptions,
      globalDirShouldAllowWrite: opts.globalDirShouldAllowWrite,
      skipGlobalBinDirCheck: opts.skipGlobalBinDirCheck,
      packageManager,
      workspaceDir: opts.workspaceDir,
      onlyInheritDlxSettingsFromLocal: opts.onlyInheritDlxSettingsFromLocal,
      forSelfUpdate: opts.forSelfUpdate,
      ignoreProjectNpmrc: opts.ignoreProjectNpmrc,
      warnings,
    })
    context.cliOptions = cliOptions
    context.rawCliConfig = opts.rawCliConfig
    applyDerivedConfig(config)

    if (opts.excludeReporter) {
      delete config.reporter // This is a silly workaround because @pnpm/installing.deps-installer expects a function as opts.reporter
    }

    return { config, context }
  } finally {
    if (opts.printWarnings !== false && warnings.length > 0) {
      console.warn(warnings.map((warning) => formatWarn(warning)).join('\n'))
    }
  }
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
  let configDependenciesVerified = false
  if (config.configDependencies) {
    configDependenciesVerified = await installConfigDeps(config, context, {
      configDependencies: config.configDependencies,
      tolerateErrors: opts?.tolerateConfigDependenciesErrors,
    })
  }
  if (config.ignorePnpmfile) {
    return { config, context }
  }
  return {
    config: await loadPnpmfileHooks(config, context, { forSelfUpdate: opts?.forSelfUpdate, configDependenciesVerified }),
    context,
  }
}

async function installConfigDeps (
  config: Config,
  context: ConfigContext,
  opts: { configDependencies: ConfigDependencies, tolerateErrors?: boolean }
): Promise<boolean> {
  const store = await createStoreController({ ...config, ...context, skipBypassedHomeStoreWarning: true })
  try {
    await resolveAndInstallConfigDeps(opts.configDependencies, {
      ...config,
      ...context,
      store: store.ctrl,
      storeDir: store.dir,
      rootDir: config.lockfileDir ?? context.rootProjectManifestDir,
      frozenLockfile: config.frozenLockfile,
    })
    return true
  } catch (err: unknown) {
    if (!opts.tolerateErrors || isConfigDependencyVerificationError(err)) {
      throw err
    }
    const errorMessage = isError(err) ? err.message : String(err)
    logger.debug({
      message: `Failed to install configDependencies. This is expected if authentication is not yet configured. Proceeding. Error: ${errorMessage}`,
      err,
    })
    return false
  } finally {
    await store.ctrl.close()
  }
}

/**
 * Loads the pnpmfiles into `context` and runs their `updateConfig` hooks.
 * Returns the config the last hook produced.
 */
async function loadPnpmfileHooks (config: Config, context: ConfigContext, opts: { forSelfUpdate?: boolean, configDependenciesVerified: boolean }): Promise<Config> {
  config.tryLoadDefaultPnpmfile = config.pnpmfile == null && !opts.forSelfUpdate
  const pnpmfiles = listConfiguredPnpmfiles(config, context, opts.configDependenciesVerified)
  const { hooks, finders, resolvedPnpmfilePaths } = await requireHooks(config.lockfileDir ?? config.dir, {
    globalPnpmfile: config.globalPnpmfile,
    pnpmfiles,
    tryLoadDefaultPnpmfile: config.tryLoadDefaultPnpmfile,
  })
  context.hooks = hooks
  context.finders = finders
  config.pnpmfile = resolvedPnpmfilePaths
  if (!context.hooks?.updateConfig?.length) {
    return config
  }
  return applyUpdateConfigHooks(config, context)
}

function listConfiguredPnpmfiles (config: Config, context: ConfigContext, configDependenciesVerified: boolean): string[] {
  const pnpmfiles = config.pnpmfile == null ? [] : Array.isArray(config.pnpmfile) ? config.pnpmfile : [config.pnpmfile]
  if (configDependenciesVerified && config.configDependencies) {
    const configModulesDir = path.join(config.lockfileDir ?? context.rootProjectManifestDir, 'node_modules/.pnpm-config')
    pnpmfiles.unshift(...calcPnpmfilePathsOfPluginDeps(configModulesDir, config.configDependencies))
  }
  return pnpmfiles
}

async function applyUpdateConfigHooks (config: Config, context: ConfigContext): Promise<Config> {
  const routingBeforeHooks = {
    registry: config.registry,
    registriesByScope: { ...config.registriesByScope },
  }
  const cliSettings = pickCliSettings(config, context.cliOptions)
  for (const updateConfig of context.hooks?.updateConfig ?? []) {
    const updateConfigResult = updateConfig(config)
    config = updateConfigResult instanceof Promise ? await updateConfigResult : updateConfigResult // eslint-disable-line no-await-in-loop -- each hook receives the config the previous one returned
  }
  applyRegistryRoutingChanges(config, routingBeforeHooks)
  restoreCliSettings(config, cliSettings)
  if (DERIVED_CONFIG_INPUTS.some((setting) => cliSettings.settings.has(setting))) {
    applyDerivedConfig(config)
  }
  return config
}

/**
 * The settings the command line set, read off `config` after the config
 * reader resolved them. The command line outranks every other layer, the
 * `updateConfig` hooks included, so these go back over whatever the hooks
 * return. `--registry` and `--@<scope>:registry` are kept as the registry
 * routes they resolved to, because a hook may replace the whole routing map.
 */
interface CliSettings {
  settings: Map<string, unknown>
  registriesByScope: Map<string, string>
}

function cloneCliSetting<Setting> (value: Setting): Setting {
  if (value === null || typeof value !== 'object') {
    return value
  }
  try {
    return structuredClone(value)
  } catch {
    if (Array.isArray(value)) {
      return value.slice() as unknown as Setting
    }
    return { ...value }
  }
}

function pickCliSettings (config: Config, cliOptions: Record<string, unknown>): CliSettings {
  const settings = new Map<string, unknown>()
  const registriesByScope = new Map<string, string>()
  for (const [key, value] of Object.entries(cliOptions)) {
    if (value === undefined) continue
    if (key.startsWith('@') && key.endsWith(':registry')) {
      const scope = key.slice(0, -':registry'.length)
      registriesByScope.set(scope, config.registriesByScope[scope])
      continue
    }
    const setting = camelcase(key, { locale: 'en-US' })
    if (Object.hasOwn(config, setting)) {
      settings.set(setting, cloneCliSetting((config as unknown as Record<string, unknown>)[setting]))
    }
    if (setting === 'registry') {
      registriesByScope.set('default', config.registriesByScope.default)
    }
  }
  return { settings, registriesByScope }
}

function restoreCliSettings (config: Config, { settings, registriesByScope }: CliSettings): void {
  for (const [setting, value] of settings) {
    (config as unknown as Record<string, unknown>)[setting] = cloneCliSetting(value)
  }
  for (const [scope, registry] of registriesByScope) {
    config.registriesByScope[scope] = registry
    if (config.packageManagerRegistries) {
      config.packageManagerRegistries[scope] = registry
    }
  }
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

interface RegistryRouting {
  registry: string | undefined
  registriesByScope: Record<string, string>
}

/**
 * Applies what the `updateConfig` hooks changed about registry routing, then re-establishes what the config
 * reader guarantees: `registriesByScope` holds normalized URLs including the `default` and `@jsr` routes, and
 * `registry` is the `default` route.
 *
 * Only a changed value counts, and a value the hooks removed is unchanged. A changed
 * `registriesByScope` replaces the scope routes, and its `default` entry, if any, the default registry. A
 * changed `registry` is applied after it, so it wins. A dropped `default` keeps the registry configured before
 * the hooks, and a dropped `@jsr` falls back to the built-in JSR registry.
 */
function applyRegistryRoutingChanges (config: Config, before: RegistryRouting): void {
  let registry = before.registry
  let routes = before.registriesByScope
  const hookRoutes: unknown = config.registriesByScope
  if (hookRoutes !== undefined) {
    const changedRoutes = readHookRegistryRoutes(hookRoutes)
    if (!equals(changedRoutes, before.registriesByScope)) {
      routes = changedRoutes
      registry = routes.default ?? registry
    }
  }
  registry = applyHookRegistry(config.registry, { before: before.registry, current: registry })
  config.registriesByScope = normalizeRegistriesByScope({
    ...routes,
    ...(registry != null ? { default: registry } : {}),
  })
  config.registry = registry === before.registry ? before.registry : config.registriesByScope.default
}

function applyHookRegistry (hookRegistry: unknown, registries: { before: string | undefined, current: string | undefined }): string | undefined {
  if (hookRegistry === registries.before || hookRegistry === undefined) return registries.current
  if (hookRegistry === null) return DEFAULT_REGISTRIES_BY_SCOPE.default
  if (typeof hookRegistry === 'string') return hookRegistry
  throw invalidHookResult('registry')
}

/** An `undefined` route is one the hook removed, as it would be once serialized. */
function readHookRegistryRoutes (routes: unknown): Record<string, string> {
  if (!isPlainObject(routes)) {
    throw invalidHookResult('registriesByScope')
  }
  const result: Record<string, string> = {}
  for (const [scope, registry] of Object.entries(routes)) {
    if (registry === undefined) continue
    if (typeof registry !== 'string') {
      throw invalidHookResult('registriesByScope')
    }
    result[scope] = registry
  }
  return result
}

function isPlainObject (value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return false
  }
  const proto = Object.getPrototypeOf(value)
  return proto === null || proto === Object.prototype
}

function invalidHookResult (key: string): PnpmError {
  return new PnpmError('INVALID_UPDATE_CONFIG_RESULT', `The updateConfig hook produced an invalid ${key} value`)
}

const DERIVED_CONFIG_INPUTS = ['hoist', 'shamefullyHoist', 'symlink']

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

function isConfigDependencyVerificationError (error: unknown): boolean {
  return isError(error) && 'code' in error && error.code === 'ERR_PNPM_BAD_CONFIG_DEP'
}
