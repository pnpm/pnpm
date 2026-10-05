import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { GLOBAL_CONFIG_YAML_FILENAME, GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { betterPathResolve } from 'better-path-resolve'
import camelcase from 'camelcase'
import { realpathMissing } from 'realpath-missing'

import type {
  Config,
  ConfigContext,
  ProjectConfig,
  UniversalOptions,
  VerifyDepsBeforeRun,
  WantedPackageManager,
} from './Config.js'
import { extractAndRemoveDependencyBuildOptions } from './dependencyBuildOptions.js'
import { getConfigDir, getDataDir } from './dirs.js'
import type { CliOptions, ConfigBuildState, PnpmConfigInProgress } from './getConfig/configBuildState.js'
import { splitConfigAndContext } from './getConfig/configContext.js'
import { createDefaultOptions, type KebabCaseConfig } from './getConfig/defaultOptions.js'
import { applyInstallSettingsFromAllSources, type DependencyBuildOptions } from './getConfig/derivedSettings.js'
import { applyEnvVarSettings, applyGlobalDirEnvVars, readEnvVar, readNpmEnvVar } from './getConfig/envVars.js'
import { finalizeConfig } from './getConfig/finalizeConfig.js'
import { applyGlobalYamlConfig } from './getConfig/globalConfigYaml.js'
import { applyGlobalInstallSettings } from './getConfig/globalInstall.js'
import { applyLocalSettings } from './getConfig/localSettings.js'
import { resolveInitialRegistries, resolveRegistriesByScope } from './getConfig/registries.js'
import { applyRuntimeSettings } from './getConfig/runtimeSettings.js'
import { assertTokenHelperComesFromTrustedConfig } from './getConfig/tokenHelper.js'
import { loadNpmrcConfig } from './loadNpmrcFiles.js'
import { inheritDlxConfig, pickIniConfig } from './localConfig.js'
import { transformGlobalDirKeys } from './transformPath.js'
import { types } from './types.js'
export { types }

export { binDirOf, modulesDirOf } from './binDir.js'
export { getDefaultWorkspaceConcurrency, getWorkspaceConcurrency } from './concurrency.js'
export { getGlobalConfigPath } from './dirs.js'
export { getDefaultCreds, getNetworkConfigs, type NetworkConfigs } from './getNetworkConfigs.js'
export { getOptionsFromPnpmSettings, type OptionsFromRootManifest, toAuditSettings, toUpdateSettings } from './getOptionsFromRootManifest.js'
export {
  getPackageManagerBootstrapConfig,
  getPackageManagerRegistries,
  type PackageManagerBootstrapConfig,
} from './packageManagerRegistries.js'
export { parseCAFileContents } from './parseCAFileContents.js'
export type { Creds } from './parseCreds.js'
export { createProjectModulesDirResolver, type ProjectModulesDirOptions } from './projectConfig.js'
export {
  createProjectConfigRecord,
  type CreateProjectConfigRecordOptions,
  ProjectConfigInvalidValueTypeError,
  ProjectConfigIsNotAnObjectError,
  ProjectConfigsArrayItemIsNotAnObjectError,
  ProjectConfigsArrayItemMatchIsNotAnArrayError,
  ProjectConfigsArrayItemMatchIsNotDefinedError,
  ProjectConfigsIsNeitherObjectNorArrayError,
  ProjectConfigsMatchItemIsNotAStringError,
  ProjectConfigUnsupportedFieldError,
} from './projectConfig.js'
export type { Config, ConfigContext, ProjectConfig, UniversalOptions, VerifyDepsBeforeRun, WantedPackageManager }

export { type ConfigFileKey, isConfigFileKey } from './configFileKey.js'
export type { CliOptions } from './getConfig/configBuildState.js'
export { getIgnoredLockfilePnpmFieldKeys } from './getConfig/ignoredPnpmFieldKeys.js'
export { type ParsedPackageManager, parsePackageManager, shouldPersistLockfile } from './getConfig/packageManager.js'
export { isProjectManifestSkippedKey, whereRefusedKeyBelongs } from './getConfig/skippedKeys.js'
export { isIniConfigKey, isNpmrcReadableKey } from './localConfig.js'

interface GetConfigOptions {
  globalDirShouldAllowWrite?: boolean
  /**
   * Skip creating the global bin directory and checking that it is on `PATH`.
   * `pnpm env remove` deletes Node copies pnpm stored for itself and must
   * run when that directory is absent or not on `PATH`.
   * Commands that link executables into the global bin still create it and check it.
   */
  skipGlobalBinDirCheck?: boolean
  cliOptions: CliOptions
  packageManager: {
    name: string
    version: string
  }
  workspaceDir?: string | undefined
  env?: Record<string, string | undefined>
  onlyInheritDlxSettingsFromLocal?: boolean
  ignoreLocalSettings?: boolean
  /** Skip the project `.npmrc`, as npm does in global mode. */
  ignoreProjectNpmrc?: boolean
  /**
   * Set by `self-update`: skip the project `pnpm-workspace.yaml`'s settings
   * that govern whether the pnpm binary may be replaced. See
   * `SELF_UPDATE_SKIPPED_SETTINGS`.
   */
  forSelfUpdate?: boolean
  /** Collects warnings as they are found, so they survive a failed load. */
  warnings?: string[]
}

interface GetConfigResult {
  config: Config
  context: ConfigContext
  warnings: string[]
}

export async function getConfig (opts: GetConfigOptions): Promise<GetConfigResult> {
  if (opts.onlyInheritDlxSettingsFromLocal) {
    return getConfigInheritingDlxSettingsFromLocal(opts)
  }

  const packageManager = opts.packageManager ?? { name: 'pnpm', version: 'undefined' }
  const cliOptions = opts.cliOptions ?? {}
  assertNoHoistConflicts(cliOptions)

  if (cliOptions.dir) {
    cliOptions.dir = await realpathMissing(cliOptions.dir)
  }
  const { configDir, state, globalDepsBuildConfig, globalYamlConfig } = await loadConfigSources(opts, cliOptions)
  const { pnpmConfig } = state
  const cwd = applyRunContext(state, { packageManager, workspaceDir: opts.workspaceDir })

  const globalYamlRegistries = applyGlobalYamlConfig(state, { configDir, globalYamlConfig })
  const initialRegistries = resolveInitialRegistries(state)
  assertTokenHelperComesFromTrustedConfig(pnpmConfig.authConfig, state.npmrcResult.trustedConfig)
  resolveGlobalDirs(state)
  pnpmConfig.dir = cwd
  if (cliOptions['global']) {
    await applyGlobalInstallSettings(state, opts)
  }
  pnpmConfig.packageManager = packageManager

  pnpmConfig.rootProjectManifestDir = pnpmConfig.lockfileDir ?? pnpmConfig.workspaceDir ?? pnpmConfig.dir
  const workspaceManifestRegistries = opts.ignoreLocalSettings
    ? undefined
    : await applyLocalSettings(state, { forSelfUpdate: opts.forSelfUpdate })
  resolveRegistriesByScope(state, { ...initialRegistries, globalYamlRegistries, workspaceManifestRegistries })

  const maxSocketsFromEnv = applyEnvVarSettings(state)
  await applyInstallSettingsFromAllSources(state, { maxSocketsFromEnv, globalDepsBuildConfig })
  applyRuntimeSettings(state)
  finalizeConfig(state)

  return { ...splitConfigAndContext(pnpmConfig as Config & ConfigContext), warnings: state.warnings }
}

async function getConfigInheritingDlxSettingsFromLocal (opts: GetConfigOptions): Promise<GetConfigResult> {
  const { onlyInheritDlxSettingsFromLocal: _, warnings = [], ...localOpts } = opts
  const globalCfgOpts: typeof localOpts = {
    ...localOpts,
    ignoreLocalSettings: true,
    cliOptions: {
      ...localOpts.cliOptions,
      dir: os.homedir(),
    },
  }
  const globalWarnings: string[] = []
  const localWarnings: string[] = []
  const results = await Promise.allSettled([
    getConfig({ ...globalCfgOpts, warnings: globalWarnings }),
    getConfig({ ...localOpts, warnings: localWarnings }),
  ])
  // Both loads read the user-level config, so they report its warnings twice.
  warnings.push(...new Set([...globalWarnings, ...localWarnings]))
  const [final, localSrc] = results.map((result) => {
    if (result.status === 'rejected') throw result.reason
    return result.value
  })
  inheritDlxConfig(final, localSrc)
  return { ...final, warnings }
}

function assertNoHoistConflicts (cliOptions: CliOptions): void {
  if (cliOptions['hoist'] !== false) return
  if (cliOptions['shamefully-hoist'] === true) {
    throw new PnpmError('CONFIG_CONFLICT_HOIST', '--shamefully-hoist cannot be used with --no-hoist')
  }
  if (cliOptions['hoist-pattern']) {
    throw new PnpmError('CONFIG_CONFLICT_HOIST', '--hoist-pattern cannot be used with --no-hoist')
  }
}

interface ConfigSources {
  configDir: string
  state: ConfigBuildState
  globalDepsBuildConfig: DependencyBuildOptions
  globalYamlConfig: WorkspaceManifest | undefined
}

/**
 * Reads the built-in defaults, the `.npmrc` files, and the command line into
 * one config. The global config.yaml is read too, but only applied later.
 */
async function loadConfigSources (opts: GetConfigOptions, cliOptions: CliOptions): Promise<ConfigSources> {
  const env = opts.env ?? process.env
  const defaultOptions = createDefaultOptions(opts.workspaceDir)
  const configDir = getConfigDir(process)
  const globalYamlConfig = await readWorkspaceManifest(configDir, GLOBAL_CONFIG_YAML_FILENAME)
  const npmrcResult = loadNpmrcConfig({
    cliOptions,
    defaultOptions: defaultOptions as Record<string, unknown>,
    dir: cliOptions.dir as string | undefined,
    workspaceDir: opts.workspaceDir,
    ignoreProjectNpmrc: opts.ignoreProjectNpmrc,
    npmrcAuthFile: getNpmrcAuthFile({ cliOptions, env, globalYamlConfig }),
    configDir: configDir as string,
    moduleDirname: import.meta.dirname,
    env: opts.env,
    // Only the global config yaml may supply `_auth` (deleted from
    // `globalYamlConfig` later so it isn't flagged as an unknown setting).
    globalConfigAuth: (globalYamlConfig as unknown as Record<string, unknown> | undefined)?._auth,
    warnings: opts.warnings,
  })

  const configFromCliOpts = Object.fromEntries(Object.entries(cliOptions)
    .filter(([_, value]) => typeof value !== 'undefined')
    .map(([name, value]) => [camelcase(name, { locale: 'en-US' }), value])
  )
  const pnpmConfig = createConfigFromDefaultsAndNpmrc(defaultOptions, npmrcResult.mergedConfig)
  const globalDepsBuildConfig = extractAndRemoveDependencyBuildOptions(pnpmConfig)

  const explicitlySetKeys = new Set<string>(Object.keys(configFromCliOpts))
  pnpmConfig.explicitlySetKeys = explicitlySetKeys
  pnpmConfig.cliOptions = cliOptions
  Object.assign(pnpmConfig, configFromCliOpts)
  pnpmConfig.configDir = configDir
  const state: ConfigBuildState = {
    cliOptions,
    configFromCliOpts,
    env,
    explicitlySetKeys,
    npmrcResult,
    pnpmConfig,
    registrySetOnCommandLine: explicitlySetKeys.has('registry'),
    warnings: npmrcResult.warnings,
  }
  return { configDir, state, globalDepsBuildConfig, globalYamlConfig }
}

/**
 * Read npmrcAuthFile early from global config.yaml (before loading .npmrc files).
 * The general env var loop runs later (after .npmrc files are loaded), so we
 * also have to peek at the relevant env vars here in order for
 * PNPM_CONFIG_NPMRC_AUTH_FILE / PNPM_CONFIG_USERCONFIG (and their lowercase
 * equivalents) to actually decide which user-level .npmrc gets read.
 * npm_config_userconfig is honored as a low-priority compatibility fallback
 * so that environments that point npm at a custom .npmrc (e.g. actions/setup-node
 * writing to ${runner.temp}/.npmrc) keep working without requiring users to
 * rename the env var to its PNPM_CONFIG_* equivalent.
 */
function getNpmrcAuthFile ({ cliOptions, env, globalYamlConfig }: {
  cliOptions: CliOptions
  env: Record<string, string | undefined>
  globalYamlConfig: WorkspaceManifest | undefined
}): string | undefined {
  return cliOptions['npmrc-auth-file'] as string | undefined
    ?? cliOptions.userconfig as string | undefined
    ?? readEnvVar(env, 'npmrc_auth_file')
    ?? readEnvVar(env, 'userconfig')
    ?? globalYamlConfig?.npmrcAuthFile
    ?? readNpmEnvVar(env, 'userconfig')
}

/** Builds the initial config from defaults, then overlays auth/registry values from .npmrc. */
function createConfigFromDefaultsAndNpmrc (
  defaultOptions: Partial<KebabCaseConfig>,
  npmrcConfig: Record<string, unknown>
): PnpmConfigInProgress {
  const pnpmConfig = Object.fromEntries(
    Object.entries(defaultOptions)
      .map(([key, value]) => [camelcase(key, { locale: 'en-US' }), value])
  ) as unknown as PnpmConfigInProgress

  for (const [key, value] of Object.entries(npmrcConfig)) {
    if (Object.hasOwn(types, key)) {
      ;(pnpmConfig as unknown as Record<string, unknown>)[camelcase(key, { locale: 'en-US' })] = value
    }
  }
  return pnpmConfig
}

/** Records where and as what pnpm runs. Returns the resolved working directory. */
function applyRunContext (
  { cliOptions, npmrcResult, pnpmConfig, warnings }: ConfigBuildState,
  { packageManager, workspaceDir }: { packageManager: GetConfigOptions['packageManager'], workspaceDir: string | undefined }
): string {
  // Resolving the current working directory to its actual location is crucial.
  // This prevents potential inconsistencies in the future, especially when processing or mapping subdirectories.
  const cwd = fs.realpathSync(betterPathResolve(cliOptions.dir ?? npmrcResult.localPrefix))

  // Unfortunately, there is no way to escape the PATH delimiter,
  // so directories added to PATH should not contain it.
  if (cwd.includes(path.delimiter)) {
    warnings.push(`Directory "${cwd}" contains the path delimiter character (${path.delimiter}), so binaries from node_modules/.bin will not be accessible via PATH. Consider renaming the directory.`)
  }

  pnpmConfig.workspaceDir = workspaceDir
  pnpmConfig.workspaceRoot = cliOptions['workspace-root'] as boolean // This is needed to prevent pnpm reading workspaceRoot from env variables

  pnpmConfig.userAgent = (cliOptions['user-agent'] as string | undefined)
    ?? `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  pnpmConfig.authConfig = pickIniConfig(npmrcResult.rawConfig)
  return cwd
}

function resolveGlobalDirs (state: ConfigBuildState): void {
  const { pnpmConfig } = state
  pnpmConfig.pnpmHomeDir = getDataDir({ env: state.env, platform: process.platform })
  applyGlobalDirEnvVars(state)
  transformGlobalDirKeys(pnpmConfig, os.homedir())
  const globalDirRoot = pnpmConfig.globalDir ? pnpmConfig.globalDir : path.join(pnpmConfig.pnpmHomeDir, 'global')
  pnpmConfig.globalPkgDir = path.join(globalDirRoot, GLOBAL_LAYOUT_VERSION)
}
