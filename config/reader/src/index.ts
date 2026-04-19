import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { createMatcher } from '@pnpm/config.matcher'
import { GLOBAL_CONFIG_YAML_FILENAME, GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { getCurrentBranch } from '@pnpm/network.git-utils'
import { applyRuntimeOnFailOverride } from '@pnpm/pkg-manifest.utils'
import { isCamelCase } from '@pnpm/text.naming-cases'
import type { DevEngines, EngineDependency, ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { betterPathResolve } from 'better-path-resolve'
import camelcase from 'camelcase'
import { isCI } from 'ci-info'
import isWindows from 'is-windows'
import kebabCase from 'lodash.kebabcase'
import normalizeRegistryUrl from 'normalize-registry-url'
import { pathAbsolute } from 'path-absolute'
import { omit } from 'ramda'
import { realpathMissing } from 'realpath-missing'
import semver from 'semver'

import { checkGlobalBinDir } from './checkGlobalBinDir.js'
import { getDefaultWorkspaceConcurrency, getWorkspaceConcurrency } from './concurrency.js'
import type {
  Config,
  ConfigContext,
  ConfigWithDeprecatedSettings,
  ProjectConfig,
  UniversalOptions,
  VerifyDepsBeforeRun,
  WantedPackageManager,
} from './Config.js'
import { isConfigFileKey } from './configFileKey.js'
import { extractAndRemoveDependencyBuildOptions, hasDependencyBuildOptions } from './dependencyBuildOptions.js'
import { getCacheDir, getConfigDir, getDataDir, getStateDir } from './dirs.js'
import { parseEnvVars } from './env.js'
import { getDefaultCreds, getNetworkConfigs } from './getNetworkConfigs.js'
import { getOptionsFromPnpmSettings } from './getOptionsFromRootManifest.js'
import { loadNpmrcConfig } from './loadNpmrcFiles.js'
import { inheritDlxConfig, pickIniConfig } from './localConfig.js'
import { npmDefaults } from './npmDefaults.js'
import {
  type CliOptions as SupportedArchitecturesCliOptions,
  overrideSupportedArchitecturesWithCLI,
} from './overrideSupportedArchitecturesWithCLI.js'
import { transformPathKeys } from './transformPath.js'
import { types } from './types.js'
export { types }

export { getDefaultWorkspaceConcurrency, getWorkspaceConcurrency } from './concurrency.js'
export { getOptionsFromPnpmSettings, type OptionsFromRootManifest } from './getOptionsFromRootManifest.js'
export type { Creds } from './parseCreds.js'
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
export { isIniConfigKey, isNpmrcReadableKey } from './localConfig.js'

type CamelToKebabCase<S extends string> = S extends `${infer T}${infer U}`
  ? `${T extends Capitalize<T> ? '-' : ''}${Lowercase<T>}${CamelToKebabCase<U>}`
  : S

type KebabCaseConfig = {
  [K in keyof ConfigWithDeprecatedSettings as CamelToKebabCase<K>]: ConfigWithDeprecatedSettings[K];
}

export type CliOptions = Record<string, unknown> & SupportedArchitecturesCliOptions & { dir?: string, json?: boolean }

export async function getConfig (opts: {
  globalDirShouldAllowWrite?: boolean
  cliOptions: CliOptions
  packageManager: {
    name: string
    version: string
  }
  workspaceDir?: string | undefined
  env?: Record<string, string | undefined>
  onlyInheritDlxSettingsFromLocal?: boolean
  ignoreLocalSettings?: boolean
}): Promise<{ config: Config, context: ConfigContext, warnings: string[] }> {
  if (opts.onlyInheritDlxSettingsFromLocal) {
    const { onlyInheritDlxSettingsFromLocal: _, ...localOpts } = opts
    const globalCfgOpts: typeof localOpts = {
      ...localOpts,
      ignoreLocalSettings: true,
      cliOptions: {
        ...localOpts.cliOptions,
        dir: os.homedir(),
      },
    }
    const [final, localSrc] = await Promise.all([getConfig(globalCfgOpts), getConfig(localOpts)])
    inheritDlxConfig(final, localSrc)
    final.warnings.push(...localSrc.warnings)
    return final
  }

  const env = opts.env ?? process.env
  const packageManager = opts.packageManager ?? { name: 'pnpm', version: 'undefined' }
  const cliOptions = opts.cliOptions ?? {}

  if (cliOptions['hoist'] === false) {
    if (cliOptions['shamefully-hoist'] === true) {
      throw new PnpmError('CONFIG_CONFLICT_HOIST', '--shamefully-hoist cannot be used with --no-hoist')
    }
    if (cliOptions['hoist-pattern']) {
      throw new PnpmError('CONFIG_CONFLICT_HOIST', '--hoist-pattern cannot be used with --no-hoist')
    }
  }

  if (cliOptions.dir) {
    cliOptions.dir = await realpathMissing(cliOptions.dir)
  }
  const defaultOptions: Partial<KebabCaseConfig> = {
    'auto-install-peers': true,
    bail: true,
    'catalog-mode': 'manual',
    ci: isCI,
    color: 'auto',
    'dangerously-allow-all-builds': false,
    'deploy-all-files': false,
    'dedupe-peer-dependents': true,
    'dedupe-peers': false,
    'dedupe-direct-deps': false,
    'dedupe-injected-deps': true,
    'disallow-workspace-cycles': false,
    'enable-modules-dir': true,
    'enable-pre-post-scripts': true,
    'exclude-links-from-lockfile': false,
    'extend-node-path': true,
    'fail-if-no-match': false,
    'fetch-retries': 2,
    'fetch-retry-factor': 10,
    'fetch-retry-maxtimeout': 60000,
    'fetch-retry-mintimeout': 10000,
    'fetch-timeout': 60000,
    'fetch-warn-timeout-ms': 10_000, // 10 sec
    'fetch-min-speed-ki-bps': 50, // 50 KiB/s
    'force-legacy-deploy': false,
    'git-shallow-hosts': [
      // Follow https://github.com/npm/git/blob/1e1dbd26bd5b87ca055defecc3679777cb480e2a/lib/clone.js#L13-L19
      'github.com',
      'gist.github.com',
      'gitlab.com',
      'bitbucket.com',
      'bitbucket.org',
    ],
    'git-branch-lockfile': false,
    hoist: true,
    'hoist-pattern': ['*'],
    'hoist-workspace-packages': true,
    'ignore-workspace-cycles': false,
    'ignore-workspace-root-check': false,
    'optimistic-repeat-install': true,
    optional: true,
    'init-package-manager': true,
    'init-type': 'module',
    'inject-workspace-packages': false,
    'link-workspace-packages': false,
    'lockfile-include-tarball-url': false,
    'minimum-release-age': 24 * 60, // 1 day
    'minimum-release-age-ignore-missing-time': true,
    'modules-cache-max-age': 7 * 24 * 60, // 7 days
    'dlx-cache-max-age': 24 * 60, // 1 day
    'node-linker': 'isolated',
    'package-lock': npmDefaults['package-lock'],
    pending: false,
    'prefer-workspace-packages': false,
    'public-hoist-pattern': [],
    'recursive-install': true,
    registry: npmDefaults.registry,
    'block-exotic-subdeps': true,
    'resolution-mode': 'highest',
    'resolve-peers-from-workspace-root': true,
    'save-peer': false,
    'save-catalog-name': undefined,
    'save-workspace-protocol': 'rolling',
    'scripts-prepend-node-path': false,
    'strict-dep-builds': true,
    'side-effects-cache': true,
    symlink: true,
    'shared-workspace-lockfile': true,
    'shell-emulator': false,
    'strict-store-pkg-content-check': true,
    reverse: false,
    sort: true,
    'strict-peer-dependencies': false,
    'unsafe-perm': npmDefaults['unsafe-perm'],
    'use-beta-cli': false,
    userconfig: npmDefaults.userconfig,
    'verify-deps-before-run': 'install',
    'verify-store-integrity': true,
    'workspace-concurrency': getDefaultWorkspaceConcurrency(),
    'workspace-prefix': opts.workspaceDir,
    'embed-readme': false,
    'registry-supports-time-field': false,
    'virtual-store-dir-max-length': isWindows() ? 60 : 120,
    'virtual-store-only': false,
    'peers-suffix-max-length': 1000,
  }

  const configDir = getConfigDir(process)

  // Read npmrcAuthFile early from global config.yaml (before loading .npmrc files)
  const globalYamlConfigForNpmrcAuthFile = await readWorkspaceManifest(configDir, GLOBAL_CONFIG_YAML_FILENAME)
  const npmrcAuthFile = cliOptions['npmrc-auth-file'] as string | undefined
    ?? cliOptions.userconfig as string | undefined
    ?? globalYamlConfigForNpmrcAuthFile?.npmrcAuthFile

  const npmrcResult = loadNpmrcConfig({
    cliOptions,
    defaultOptions: defaultOptions as Record<string, unknown>,
    dir: cliOptions.dir as string | undefined,
    workspaceDir: opts.workspaceDir,
    npmrcAuthFile,
    configDir: configDir as string,
    moduleDirname: import.meta.dirname,
    env: opts.env,
  })
  const warnings = npmrcResult.warnings

  const configFromCliOpts = Object.fromEntries(Object.entries(cliOptions)
    .filter(([_, value]) => typeof value !== 'undefined')
    .map(([name, value]) => [camelcase(name, { locale: 'en-US' }), value])
  )

  // Build initial config from defaults, then overlay auth/registry values from .npmrc
  const pnpmConfig = Object.fromEntries(
    Object.entries(defaultOptions)
      .map(([key, value]) => [camelcase(key, { locale: 'en-US' }), value])
  ) as unknown as (ConfigWithDeprecatedSettings & ConfigContext)

  for (const [key, value] of Object.entries(npmrcResult.mergedConfig)) {
    if (Object.hasOwn(types, key)) {
      ;(pnpmConfig as unknown as Record<string, unknown>)[camelcase(key, { locale: 'en-US' })] = value
    }
  }

  const globalDepsBuildConfig = extractAndRemoveDependencyBuildOptions(pnpmConfig)

  // Track which keys are explicitly set (not defaults)
  const explicitlySetKeys = new Set<string>(Object.keys(configFromCliOpts))
  pnpmConfig.explicitlySetKeys = explicitlySetKeys
  pnpmConfig.cliOptions = cliOptions

  Object.assign(pnpmConfig, configFromCliOpts)
  // Resolving the current working directory to its actual location is crucial.
  // This prevents potential inconsistencies in the future, especially when processing or mapping subdirectories.
  const cwd = fs.realpathSync(betterPathResolve(cliOptions.dir ?? npmrcResult.localPrefix))

  // Unfortunately, there is no way to escape the PATH delimiter,
  // so directories added to PATH should not contain it.
  if (cwd.includes(path.delimiter)) {
    warnings.push(`Directory "${cwd}" contains the path delimiter character (${path.delimiter}), so binaries from node_modules/.bin will not be accessible via PATH. Consider renaming the directory.`)
  }

  // @ts-expect-error - maxsockets (lowercase) comes from npmConfigTypes, maxSockets (camelCase) is the Config field
  pnpmConfig.maxSockets = pnpmConfig.maxSockets ?? pnpmConfig['maxsockets'] ?? npmDefaults.maxsockets
  // @ts-expect-error
  delete pnpmConfig['maxsockets']

  pnpmConfig.configDir = configDir
  pnpmConfig.workspaceDir = opts.workspaceDir
  pnpmConfig.workspaceRoot = cliOptions['workspace-root'] as boolean // This is needed to prevent pnpm reading workspaceRoot from env variables

  pnpmConfig.userAgent = (cliOptions['user-agent'] as string | undefined)
    ?? `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  pnpmConfig.authConfig = pickIniConfig(npmrcResult.rawConfig)

  // Reuse the global config.yaml already read for npmrcAuthFile
  const globalYamlConfig = globalYamlConfigForNpmrcAuthFile
  if (globalYamlConfig) {
    for (const key in globalYamlConfig) {
      if (!isConfigFileKey(kebabCase(key))) {
        delete globalYamlConfig[key as keyof typeof globalYamlConfig]
      }
    }
    addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
      configFromCliOpts,
      projectManifest: undefined,
      workspaceDir: undefined,
      workspaceManifest: globalYamlConfig,
    })
  }
  const networkConfigs = getNetworkConfigs(pnpmConfig.authConfig)
  const registriesFromNpmrc = {
    default: normalizeRegistryUrl(pnpmConfig.authConfig.registry),
    ...networkConfigs.registries,
  }
  pnpmConfig.registries = { ...registriesFromNpmrc }
  const defaultCreds = getDefaultCreds(pnpmConfig.authConfig)
  pnpmConfig.configByUri = {
    ...networkConfigs.configByUri,
    ...defaultCreds ? { '': { creds: defaultCreds } } : {},
  }
  // tokenHelper must only come from user-level config (~/.npmrc or global auth.ini),
  // not project-level, to prevent project .npmrc from executing arbitrary commands.
  const userConfig = npmrcResult.userConfig as Record<string, string>
  for (const [key, value] of Object.entries(pnpmConfig.authConfig)) {
    if (!key.endsWith('tokenHelper') && key !== 'tokenHelper') continue
    if (!(key in userConfig) || userConfig[key] !== value) {
      throw new PnpmError('TOKEN_HELPER_IN_PROJECT_CONFIG',
        'tokenHelper must not be configured in project-level .npmrc',
        { hint: `The key "${key}" was found in project config. Move it to ~/.npmrc or the global pnpm auth.ini.` })
    }
  }
  pnpmConfig.pnpmHomeDir = getDataDir({ env, platform: process.platform })
  let globalDirRoot
  if (pnpmConfig.globalDir) {
    globalDirRoot = pnpmConfig.globalDir
  } else {
    globalDirRoot = path.join(pnpmConfig.pnpmHomeDir, 'global')
  }
  pnpmConfig.globalPkgDir = path.join(globalDirRoot, GLOBAL_LAYOUT_VERSION)
  pnpmConfig.dir = cwd
  if (cliOptions['global']) {
    delete pnpmConfig.workspaceDir
    pnpmConfig.bin = pnpmConfig.globalBinDir ?? path.join(pnpmConfig.pnpmHomeDir, 'bin')
    if (pnpmConfig.bin) {
      fs.mkdirSync(pnpmConfig.bin, { recursive: true })
      await checkGlobalBinDir(pnpmConfig.bin, { env, shouldAllowWrite: opts.globalDirShouldAllowWrite })
    }
    pnpmConfig.save = true
    pnpmConfig.allowNew = true
    pnpmConfig.ignoreCurrentSpecifiers = true
    pnpmConfig.saveProd = true
    pnpmConfig.saveDev = false
    pnpmConfig.saveOptional = false
    if ((pnpmConfig.hoistPattern != null) && (pnpmConfig.hoistPattern.length > 1 || pnpmConfig.hoistPattern[0] !== '*')) {
      if (opts.cliOptions['hoist-pattern']) {
        throw new PnpmError('CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL',
          'Configuration conflict. "hoist-pattern" may not be used with "global"')
      }
    }
    if (pnpmConfig.linkWorkspacePackages) {
      if (opts.cliOptions['link-workspace-packages']) {
        throw new PnpmError('CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL',
          'Configuration conflict. "link-workspace-packages" may not be used with "global"')
      }
      pnpmConfig.linkWorkspacePackages = false
    }
    if (pnpmConfig.sharedWorkspaceLockfile) {
      if (opts.cliOptions['shared-workspace-lockfile']) {
        throw new PnpmError('CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL',
          'Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
      }
      pnpmConfig.sharedWorkspaceLockfile = false
    }
    if (pnpmConfig.lockfileDir) {
      if (opts.cliOptions['lockfile-dir']) {
        throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL',
          'Configuration conflict. "lockfile-dir" may not be used with "global"')
      }
      delete pnpmConfig.lockfileDir
    }
    if (opts.cliOptions['virtual-store-dir']) {
      throw new PnpmError('CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL',
        'Configuration conflict. "virtual-store-dir" may not be used with "global"')
    }
    if (pnpmConfig.enableGlobalVirtualStore == null) {
      pnpmConfig.enableGlobalVirtualStore = true
    }
  } else if (!pnpmConfig.bin) {
    pnpmConfig.bin = path.join(pnpmConfig.dir, 'node_modules', '.bin')
  }
  // Default allowBuilds to {} when GVS is enabled so that GVS hashes
  // are engine-agnostic when no build policy is configured. Without
  // this, allowBuilds is undefined which makes createAllowBuildFunction
  // return undefined, causing all hashes to include ENGINE_NAME.
  if (pnpmConfig.enableGlobalVirtualStore && pnpmConfig.allowBuilds == null) {
    pnpmConfig.allowBuilds = {}
  }
  pnpmConfig.packageManager = packageManager

  pnpmConfig.rootProjectManifestDir = pnpmConfig.lockfileDir ?? pnpmConfig.workspaceDir ?? pnpmConfig.dir
  if (!opts.ignoreLocalSettings) {
    pnpmConfig.rootProjectManifest = await safeReadProjectManifestOnly(pnpmConfig.rootProjectManifestDir) ?? undefined
    if (pnpmConfig.rootProjectManifest != null) {
      if (pnpmConfig.rootProjectManifest.workspaces?.length && !pnpmConfig.workspaceDir) {
        warnings.push('The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead.')
      }
      const wantedPmResult = getWantedPackageManager(pnpmConfig.rootProjectManifest)
      if (wantedPmResult.pm) {
        pnpmConfig.wantedPackageManager = wantedPmResult.pm
      }
      warnings.push(...wantedPmResult.warnings)
      if (pnpmConfig.nodeVersion == null) {
        pnpmConfig.nodeVersion = getNodeVersionFromEnginesRuntime(pnpmConfig.rootProjectManifest)
      }
    }

    if (pnpmConfig.workspaceDir != null) {
      const workspaceManifest = await readWorkspaceManifest(pnpmConfig.workspaceDir)

      pnpmConfig.workspacePackagePatterns = cliOptions['workspace-packages'] as string[] ?? workspaceManifest?.packages ?? ['.']
      if (workspaceManifest) {
        addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
          configFromCliOpts,
          projectManifest: pnpmConfig.rootProjectManifest,
          workspaceDir: pnpmConfig.workspaceDir,
          workspaceManifest,
        })
      }
    } else if (cliOptions['global']) {
      // For global installs, read settings from pnpm-workspace.yaml in the global package directory
      const workspaceManifest = await readWorkspaceManifest(pnpmConfig.globalPkgDir)
      if (workspaceManifest) {
        addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
          configFromCliOpts,
          projectManifest: pnpmConfig.rootProjectManifest,
          workspaceDir: pnpmConfig.globalPkgDir,
          workspaceManifest,
        })
      }
    }
  }

  // Merge registries from pnpm-workspace.yaml onto the .npmrc-based registries.
  // The workspace manifest may have set pnpmConfig.registries via addSettingsFromWorkspaceManifestToConfig,
  // but we need to ensure 'default' is always set and all URLs are normalized.
  const workspaceRegistries = pnpmConfig.registries as Record<string, string> | undefined
  pnpmConfig.registries = {
    ...registriesFromNpmrc,
    ...workspaceRegistries,
  }
  if (!pnpmConfig.registries.default) {
    pnpmConfig.registries.default = registriesFromNpmrc.default
  }
  for (const [scope, url] of Object.entries(pnpmConfig.registries)) {
    if (typeof url === 'string') {
      pnpmConfig.registries[scope] = normalizeRegistryUrl(url)
    }
  }

  // omit some schema that the custom parser can't yet handle
  const envPnpmTypes = omit([
    'init-version', // the type is a private function named 'semver'
    'node-version', // the type is a private function named 'semver'
    'umask', // the type is a private function named 'Umask'
  ], types)

  for (const { key, value } of parseEnvVars(key => envPnpmTypes[key as keyof typeof envPnpmTypes], env)) {
    // undefined means that the env key was defined, but its value couldn't be parsed according to the schema
    // TODO: should we throw some error or print some warning here?
    if (value === undefined) continue

    if (Object.hasOwn(cliOptions, key) || Object.hasOwn(cliOptions, kebabCase(key))) continue

    // @ts-expect-error
    pnpmConfig[key] = value
    explicitlySetKeys.add(key)

    if (key === 'registry') {
      if (typeof value !== 'string') {
        throw new TypeError(`Unexpected type of registry, expecting a string but received ${JSON.stringify(value)}`)
      }
      pnpmConfig.registries.default = normalizeRegistryUrl(value)
    }
  }

  overrideSupportedArchitecturesWithCLI(pnpmConfig, cliOptions)

  pnpmConfig.useLockfile = (() => {
    if (typeof pnpmConfig.lockfile === 'boolean') return pnpmConfig.lockfile
    if (typeof pnpmConfig.packageLock === 'boolean') return pnpmConfig.packageLock
    return false
  })()

  pnpmConfig.useGitBranchLockfile = (() => {
    if (typeof pnpmConfig.gitBranchLockfile === 'boolean') return pnpmConfig.gitBranchLockfile
    return false
  })()
  pnpmConfig.mergeGitBranchLockfiles = await (async () => {
    if (typeof pnpmConfig.mergeGitBranchLockfiles === 'boolean') return pnpmConfig.mergeGitBranchLockfiles
    if (pnpmConfig.mergeGitBranchLockfilesBranchPattern != null && pnpmConfig.mergeGitBranchLockfilesBranchPattern.length > 0) {
      const branch = await getCurrentBranch()
      if (branch) {
        const branchMatcher = createMatcher(pnpmConfig.mergeGitBranchLockfilesBranchPattern)
        return branchMatcher(branch)
      }
    }
    return undefined
  })()

  if (!hasDependencyBuildOptions(pnpmConfig)) {
    Object.assign(pnpmConfig, globalDepsBuildConfig)
  }
  if (opts.cliOptions['save-peer']) {
    if (opts.cliOptions['save-prod']) {
      throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP', 'A package cannot be a peer dependency and a prod dependency at the same time')
    }
    if (opts.cliOptions['save-optional']) {
      throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP',
        'A package cannot be a peer dependency and an optional dependency at the same time')
    }
  }
  if (typeof pnpmConfig.filter === 'string') {
    pnpmConfig.filter = (pnpmConfig.filter as string).split(' ')
  }

  if (typeof pnpmConfig.filterProd === 'string') {
    pnpmConfig.filterProd = (pnpmConfig.filterProd as string).split(' ')
  }

  if (pnpmConfig.workspaceDir) {
    pnpmConfig.extraBinPaths = [path.join(pnpmConfig.workspaceDir, 'node_modules', '.bin')]
  } else {
    pnpmConfig.extraBinPaths = []
  }

  pnpmConfig.extraEnv = {
    pnpm_config_verify_deps_before_run: 'false',
  }
  if (pnpmConfig.preferSymlinkedExecutables && !isWindows()) {
    const cwd = pnpmConfig.lockfileDir ?? pnpmConfig.dir

    const virtualStoreDir = pnpmConfig.virtualStoreDir
      ? pnpmConfig.virtualStoreDir
      : pnpmConfig.modulesDir
        ? path.join(pnpmConfig.modulesDir, '.pnpm')
        : 'node_modules/.pnpm'

    pnpmConfig.extraEnv['NODE_PATH'] = pathAbsolute(path.join(virtualStoreDir, 'node_modules'), cwd)
  }

  if (!pnpmConfig.cacheDir) {
    pnpmConfig.cacheDir = getCacheDir(process)
  }
  if (!pnpmConfig.stateDir) {
    pnpmConfig.stateDir = getStateDir(process)
  }
  if (typeof pnpmConfig['color'] === 'boolean') {
    switch (pnpmConfig['color']) {
      case true:
        pnpmConfig.color = 'always'
        break
      case false:
        pnpmConfig.color = 'never'
        break
      default:
        pnpmConfig.color = 'auto'
        break
    }
  }
  if (!pnpmConfig.httpsProxy) {
    pnpmConfig.httpsProxy = pnpmConfig.proxy ?? getProcessEnv('https_proxy')
  }
  if (!pnpmConfig.httpProxy) {
    pnpmConfig.httpProxy = pnpmConfig.httpsProxy ?? getProcessEnv('http_proxy') ?? getProcessEnv('proxy')
  }
  if (!pnpmConfig.noProxy) {
    // @ts-expect-error
    pnpmConfig.noProxy = pnpmConfig['noproxy'] ?? getProcessEnv('no_proxy')
  }
  switch (pnpmConfig.nodeLinker) {
    case 'pnp':
      pnpmConfig.enablePnp = pnpmConfig.nodeLinker === 'pnp'
      break
    case 'hoisted':
      if (pnpmConfig.preferSymlinkedExecutables == null) {
        pnpmConfig.preferSymlinkedExecutables = true
      }
      break
  }
  if (!pnpmConfig.userConfig) {
    pnpmConfig.userConfig = npmrcResult.userConfig as Record<string, string>
  }
  pnpmConfig.sideEffectsCacheRead = pnpmConfig.sideEffectsCache ?? pnpmConfig.sideEffectsCacheReadonly
  pnpmConfig.sideEffectsCacheWrite = pnpmConfig.sideEffectsCache

  if (pnpmConfig.sharedWorkspaceLockfile && !pnpmConfig.lockfileDir && pnpmConfig.workspaceDir) {
    pnpmConfig.lockfileDir = pnpmConfig.workspaceDir
  }

  pnpmConfig.workspaceConcurrency = getWorkspaceConcurrency(pnpmConfig.workspaceConcurrency)

  if (pnpmConfig.only === 'prod' || pnpmConfig.only === 'production' || !pnpmConfig.only && pnpmConfig.production) {
    pnpmConfig.production = true
    pnpmConfig.dev = false
  } else if (pnpmConfig.only === 'dev' || pnpmConfig.only === 'development' || pnpmConfig.dev) {
    pnpmConfig.production = false
    pnpmConfig.dev = true
    pnpmConfig.optional = false
  } else {
    pnpmConfig.production = true
    pnpmConfig.dev = true
  }

  if (pnpmConfig.ci && pnpmConfig.enableGlobalVirtualStore == null) {
    // Using a global virtual store in CI makes little sense,
    // as there is usually no warm cache in that environment.
    // However, if the user explicitly enabled GVS (e.g., for Nix builds
    // or CI systems with persistent caches), respect that setting.
    pnpmConfig.enableGlobalVirtualStore = false
  }

  // The yes option is only meant to be a CLI option. Remove it from the
  // returned pnpm config.
  delete (pnpmConfig as { yes?: boolean }).yes
  if (cliOptions.yes) {
    pnpmConfig.autoConfirmAllPrompts = true
  }

  transformPathKeys(pnpmConfig, os.homedir())

  // The `pmOnFail` config setting overrides whatever onFail the
  // wantedPackageManager carried, so users (and internal callers) can force
  // a specific behavior without editing the manifest. Otherwise, the legacy
  // `packageManager` field defaults to `download` — `devEngines.packageManager`
  // already has onFail set during parsing.
  if (pnpmConfig.wantedPackageManager) {
    if (pnpmConfig.pmOnFail) {
      pnpmConfig.wantedPackageManager.onFail = pnpmConfig.pmOnFail
    } else if (pnpmConfig.wantedPackageManager.onFail == null) {
      pnpmConfig.wantedPackageManager.onFail = 'download'
    }
  }

  if (pnpmConfig.runtimeOnFail && pnpmConfig.rootProjectManifest) {
    applyRuntimeOnFailOverride(pnpmConfig.rootProjectManifest, pnpmConfig.runtimeOnFail)
  }

  const {
    hooks, finders,
    allProjects, selectedProjectsGraph, allProjectsGraph,
    rootProjectManifest, rootProjectManifestDir,
    cliOptions: ctxCliOptions,
    explicitlySetKeys: ctxExplicitlySetKeys,
    packageManager: ctxPackageManager, wantedPackageManager,
    ...config
  } = pnpmConfig as Config & ConfigContext
  const context: ConfigContext = {
    hooks, finders,
    allProjects, selectedProjectsGraph, allProjectsGraph,
    rootProjectManifest, rootProjectManifestDir,
    cliOptions: ctxCliOptions,
    explicitlySetKeys: ctxExplicitlySetKeys,
    packageManager: ctxPackageManager, wantedPackageManager,
  }
  return { config, context, warnings }
}

function getProcessEnv (env: string): string | undefined {
  return process.env[env] ??
    process.env[env.toUpperCase()] ??
    process.env[env.toLowerCase()]
}

function getWantedPackageManager (manifest: ProjectManifest): { pm?: WantedPackageManager, warnings: string[] } {
  const warnings: string[] = []
  const pmFromDevEngines = parseDevEnginesPackageManager(manifest.devEngines)
  if (pmFromDevEngines) {
    if (pmFromDevEngines.version != null && !semver.validRange(pmFromDevEngines.version)) {
      warnings.push(`Cannot use devEngines.packageManager version "${pmFromDevEngines.version}": not a valid version or range`)
      pmFromDevEngines.version = undefined
    }
    if (manifest.packageManager) {
      const legacyPm = parsePackageManager(manifest.packageManager)
      if (legacyPm.name !== pmFromDevEngines.name || legacyPm.version !== pmFromDevEngines.version) {
        warnings.push('Cannot use both "packageManager" and "devEngines.packageManager" in package.json. "packageManager" will be ignored')
      }
    }
    return { pm: { ...pmFromDevEngines, fromDevEngines: true }, warnings }
  }
  if (manifest.packageManager) {
    const pm = parsePackageManager(manifest.packageManager)
    if (pm.version != null) {
      const cleanVersion = semver.valid(pm.version)
      if (!cleanVersion) {
        warnings.push(`Cannot use packageManager "${manifest.packageManager}": "${pm.version}" is not a valid exact version`)
        pm.version = undefined
      } else if (cleanVersion !== pm.version) {
        warnings.push(`Cannot use packageManager "${manifest.packageManager}": you need to specify the version as "${cleanVersion}"`)
        pm.version = undefined
      }
    }
    return { pm, warnings }
  }
  return { warnings }
}

function parsePackageManager (packageManager: string): { name: string, version: string | undefined } {
  if (!packageManager.includes('@')) return { name: packageManager, version: undefined }
  const [name, pmReference] = packageManager.split('@')
  // pmReference is semantic versioning, not URL
  if (pmReference.includes(':')) return { name, version: undefined }
  // Remove the integrity hash. Ex: "pnpm@9.5.0+sha512.140036830124618d624a2187b50d04289d5a087f326c9edfc0ccd733d76c4f52c3a313d4fc148794a2a9d81553016004e6742e8cf850670268a7387fc220c903"
  const [version] = pmReference.split('+')
  return {
    name,
    version,
  }
}

function parseDevEnginesPackageManager (devEngines?: DevEngines): EngineDependency | undefined {
  if (!devEngines?.packageManager) return undefined
  let pmEngine: EngineDependency | undefined
  let onFail: 'ignore' | 'warn' | 'error' | 'download'
  if (Array.isArray(devEngines.packageManager)) {
    const engines = devEngines.packageManager
    if (engines.length === 0) return undefined
    const pnpmIndex = engines.findIndex((engine) => engine.name === 'pnpm')
    if (pnpmIndex !== -1) {
      pmEngine = engines[pnpmIndex]
      // In array notation, default onFail is 'error' for the last element, 'ignore' for others.
      onFail = pmEngine.onFail ?? (pnpmIndex === engines.length - 1 ? 'error' : 'ignore')
    } else {
      pmEngine = engines[0]
      // No pnpm entry found — use the last element's onFail for the overall failure behavior.
      const lastEngine = engines[engines.length - 1]
      onFail = lastEngine.onFail ?? 'error'
    }
  } else {
    pmEngine = devEngines.packageManager
    onFail = pmEngine.onFail ?? 'error'
  }
  if (!pmEngine?.name) return undefined
  return {
    name: pmEngine.name,
    version: pmEngine.version,
    onFail,
  }
}

function getNodeVersionFromEnginesRuntime (manifest: ProjectManifest): string | undefined {
  for (const enginesFieldName of ['devEngines', 'engines'] as const) {
    const enginesRuntime = manifest[enginesFieldName]?.runtime
    if (enginesRuntime == null) continue
    const runtimes: EngineDependency[] = Array.isArray(enginesRuntime) ? enginesRuntime : [enginesRuntime]
    const nodeRuntime = runtimes.find((r) => r.name === 'node')
    if (nodeRuntime?.version == null) continue
    const minVersion = semver.minVersion(nodeRuntime.version)
    if (minVersion != null) {
      return minVersion.version
    }
  }
  return undefined
}

function addSettingsFromWorkspaceManifestToConfig (pnpmConfig: Config & ConfigContext, {
  configFromCliOpts,
  projectManifest,
  workspaceManifest,
  workspaceDir,
}: {
  configFromCliOpts: Record<string, unknown>
  projectManifest: ProjectManifest | undefined
  workspaceDir: string | undefined
  workspaceManifest: WorkspaceManifest
}): void {
  const newSettings = Object.assign(getOptionsFromPnpmSettings(workspaceDir, workspaceManifest, projectManifest), configFromCliOpts)
  for (const [key, value] of Object.entries(newSettings)) {
    if (!isCamelCase(key)) continue

    // @ts-expect-error
    pnpmConfig[key] = value
    pnpmConfig.explicitlySetKeys.add(key)
  }
  // All the pnpm_config_ env variables should override the settings from pnpm-workspace.yaml,
  // as it happens with .npmrc.
  // Until that is fixed, we should at the very least keep the right priority for verifyDepsBeforeRun,
  // or else, we'll get infinite recursion.
  // Related issue: https://github.com/pnpm/pnpm/issues/10060
  if (process.env.pnpm_config_verify_deps_before_run != null) {
    pnpmConfig.verifyDepsBeforeRun = process.env.pnpm_config_verify_deps_before_run as VerifyDepsBeforeRun
  }
  pnpmConfig.catalogs = getCatalogsFromWorkspaceManifest(workspaceManifest)
}

