import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { stripVTControlCharacters } from 'node:util'

import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { createMatcher } from '@pnpm/config.matcher'
import { BUILTIN_REGISTRIES_BY_PREFIX, GLOBAL_CONFIG_YAML_FILENAME, GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { PnpmError, redactAndSanitize } from '@pnpm/error'
import { addEsmNodePathLoaderOption } from '@pnpm/exec.esm-node-path-loader'
import { getCurrentBranch } from '@pnpm/network.git-utils'
import { applyRuntimeOnFailOverride } from '@pnpm/pkg-manifest.utils'
import { isCamelCase } from '@pnpm/text.naming-cases'
import type { DevEngines, EngineDependency, ProjectManifest, VirtualStoreType } from '@pnpm/types'
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
  PackageManagerNetworkConfig,
  ProjectConfig,
  UniversalOptions,
  VerifyDepsBeforeRun,
  WantedPackageManager,
} from './Config.js'
import { isConfigFileKey } from './configFileKey.js'
import { extractAndRemoveDependencyBuildOptions, hasDependencyBuildOptions } from './dependencyBuildOptions.js'
import { getCacheDir, getConfigDir, getDataDir, getGlobalConfigPath, getStateDir } from './dirs.js'
import { parseEnvVars } from './env.js'
import { getNetworkConfigs } from './getNetworkConfigs.js'
import { getOptionsFromPnpmSettings } from './getOptionsFromRootManifest.js'
import { loadNpmrcConfig } from './loadNpmrcFiles.js'
import { inheritDlxConfig, pickIniConfig } from './localConfig.js'
import { npmDefaults } from './npmDefaults.js'
import {
  type CliOptions as SupportedArchitecturesCliOptions,
  overrideSupportedArchitecturesWithCLI,
} from './overrideSupportedArchitecturesWithCLI.js'
import { quoteAndJoin } from './quoteAndJoin.js'
import { transformPathKeys } from './transformPath.js'
import { types } from './types.js'
export { types }

export { getDefaultWorkspaceConcurrency, getWorkspaceConcurrency } from './concurrency.js'
export { getGlobalConfigPath } from './dirs.js'
export { getDefaultCreds, getNetworkConfigs, type NetworkConfigs } from './getNetworkConfigs.js'
export { getOptionsFromPnpmSettings, type OptionsFromRootManifest } from './getOptionsFromRootManifest.js'
export {
  getPackageManagerBootstrapConfig,
  getPackageManagerRegistries,
  type PackageManagerBootstrapConfig,
} from './packageManagerRegistries.js'
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
  ? `${T extends Lowercase<T> ? '' : '-'}${Lowercase<T>}${CamelToKebabCase<U>}`
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
  /**
   * Set by `self-update`: skip the project `pnpm-workspace.yaml`'s settings
   * that govern whether the pnpm binary may be replaced. See
   * {@link SELF_UPDATE_SKIPPED_SETTINGS}.
   */
  forSelfUpdate?: boolean
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
    'node-experimental-package-map': false,
    'node-package-map-type': 'standard',
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
    'frozen-store': false,
    'workspace-concurrency': getDefaultWorkspaceConcurrency(),
    'workspace-prefix': opts.workspaceDir,
    'embed-readme': false,
    'skip-manifest-obfuscation': false,
    'registry-supports-time-field': false,
    'virtual-store-dir-max-length': isWindows() ? 60 : 120,
    'virtual-store-only': false,
    'peers-suffix-max-length': 1000,
  }

  const configDir = getConfigDir(process)

  // Read npmrcAuthFile early from global config.yaml (before loading .npmrc files).
  // The general env var loop runs later (after .npmrc files are loaded), so we
  // also have to peek at the relevant env vars here in order for
  // PNPM_CONFIG_NPMRC_AUTH_FILE / PNPM_CONFIG_USERCONFIG (and their lowercase
  // equivalents) to actually decide which user-level .npmrc gets read.
  // npm_config_userconfig is honored as a low-priority compatibility fallback
  // so that environments that point npm at a custom .npmrc (e.g. actions/setup-node
  // writing to ${runner.temp}/.npmrc) keep working without requiring users to
  // rename the env var to its PNPM_CONFIG_* equivalent.
  const globalYamlConfigForNpmrcAuthFile = await readWorkspaceManifest(configDir, GLOBAL_CONFIG_YAML_FILENAME)
  const npmrcAuthFile = cliOptions['npmrc-auth-file'] as string | undefined
    ?? cliOptions.userconfig as string | undefined
    ?? readEnvVar(env, 'npmrc_auth_file')
    ?? readEnvVar(env, 'userconfig')
    ?? globalYamlConfigForNpmrcAuthFile?.npmrcAuthFile
    ?? readNpmEnvVar(env, 'userconfig')

  const npmrcResult = loadNpmrcConfig({
    cliOptions,
    defaultOptions: defaultOptions as Record<string, unknown>,
    dir: cliOptions.dir as string | undefined,
    workspaceDir: opts.workspaceDir,
    npmrcAuthFile,
    configDir: configDir as string,
    moduleDirname: import.meta.dirname,
    env: opts.env,
    // Only the global config yaml may supply `_auth` (deleted from
    // `globalYamlConfig` below so it isn't flagged as an unknown setting).
    globalConfigAuth: (globalYamlConfigForNpmrcAuthFile as unknown as Record<string, unknown> | undefined)?._auth,
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

  let globalYamlRegistries: Record<string, string> | undefined
  // Reuse the global config.yaml already read for npmrcAuthFile
  const globalYamlConfig = globalYamlConfigForNpmrcAuthFile
  if (globalYamlConfig) {
    // Consumed by loadNpmrcConfig above; drop so it isn't flagged as unknown.
    delete (globalYamlConfig as unknown as Record<string, unknown>)._auth
    const ignoredKeys: string[] = []
    // The gate below is kebab-based, but only camelCase keys are picked up later.
    const kebabKeys: string[] = []
    for (const key in globalYamlConfig) {
      if (!isConfigFileKey(kebabCase(key))) {
        ignoredKeys.push(key)
        delete globalYamlConfig[key as keyof typeof globalYamlConfig]
      } else if (!isCamelCase(key)) {
        kebabKeys.push(key)
        delete globalYamlConfig[key as keyof typeof globalYamlConfig]
      }
    }
    if (ignoredKeys.length > 0 || kebabKeys.length > 0) {
      const globalYamlConfigPath = getGlobalConfigPath(configDir)
      const movable = ignoredKeys.filter((key) => !isRefusedByAProjectManifest(key))
      const nowhere = ignoredKeys.filter(isRefusedByAProjectManifest)
      if (movable.length > 0) {
        warnings.push(`The following settings cannot be set in the global config file ("${globalYamlConfigPath}") and were ignored: ${quoteAndJoin(movable)}. Move them to a project-level pnpm-workspace.yaml. To share these settings across projects, use config dependencies: https://pnpm.io/11.x/config-dependencies`)
      }
      if (nowhere.length > 0) {
        warnings.push(`The following settings cannot be set in the global config file ("${globalYamlConfigPath}") and were ignored: ${quoteAndExplain(nowhere)}.`)
      }
      if (kebabKeys.length > 0) {
        warnings.push(`The following settings in the global config file ("${globalYamlConfigPath}") were ignored because they are not written in camelCase: ${kebabKeys.map(k => `"${k}" (use "${camelcase(k)}")`).join(', ')}.`)
      }
    }
    addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
      configFromCliOpts,
      expandRequestDestinationEnv: true,
      projectManifest: undefined,
      skipSettings: GLOBAL_CONFIG_SKIPPED_KEYS,
      workspaceDir: undefined,
      workspaceManifest: globalYamlConfig,
    })
    globalYamlRegistries = pnpmConfig.registriesByScope as Record<string, string> | undefined
  }
  const networkConfigs = getNetworkConfigs(pnpmConfig.authConfig)
  const registriesFromNpmrc = {
    default: normalizeRegistryUrl(pnpmConfig.authConfig.registry),
    ...networkConfigs.registries,
  }
  const trustedAuthConfig = pickIniConfig(npmrcResult.trustedConfig)
  const trustedNetworkConfigs = getNetworkConfigs(trustedAuthConfig)
  const cliScopedRegistries: Record<string, string> = {}
  for (const [key, value] of Object.entries(cliOptions)) {
    if (key.startsWith('@') && key.endsWith(':registry') && typeof value === 'string') {
      cliScopedRegistries[key.slice(0, -':registry'.length)] = normalizeRegistryUrl(value)
    }
  }
  pnpmConfig.registriesByScope = { ...registriesFromNpmrc }
  if (explicitlySetKeys.has('registry') && typeof pnpmConfig.registry === 'string') {
    pnpmConfig.registriesByScope.default = normalizeRegistryUrl(pnpmConfig.registry)
  }
  pnpmConfig.packageManagerRegistries = {
    default: normalizeRegistryUrl(trustedAuthConfig.registry as string),
    ...trustedNetworkConfigs.registries,
    // `_auth` routes apply here too so bootstrap (self-download / version
    // switching) resolves the same way as regular installs.
    ...npmrcResult.jsonAuth.registries,
    ...cliScopedRegistries,
  }
  if (explicitlySetKeys.has('registry') && typeof pnpmConfig.registry === 'string') {
    pnpmConfig.packageManagerRegistries.default = normalizeRegistryUrl(pnpmConfig.registry)
  }
  pnpmConfig.packageManagerNetworkConfig = createPackageManagerNetworkConfig(
    npmrcResult.trustedConfig,
    trustedNetworkConfigs.configByUri ?? {},
    env
  )
  pnpmConfig.configByUri = { ...networkConfigs.configByUri }

  // tokenHelper names an executable pnpm runs, so it must only come from trusted,
  // non-repo config sources (~/.npmrc and the global auth.ini) — never from a
  // workspace or project .npmrc, which could otherwise execute arbitrary commands.
  // trustedConfig merges exactly those trusted sources and excludes the repo ones.
  const trustedConfig = npmrcResult.trustedConfig as Record<string, string>
  for (const [key, value] of Object.entries(pnpmConfig.authConfig)) {
    if (!key.endsWith('tokenHelper') && key !== 'tokenHelper') continue
    if (!(key in trustedConfig) || trustedConfig[key] !== value) {
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
  pnpmConfig.packageManager = packageManager

  pnpmConfig.rootProjectManifestDir = pnpmConfig.lockfileDir ?? pnpmConfig.workspaceDir ?? pnpmConfig.dir
  let workspaceManifestRegistries: Record<string, string> | undefined
  if (!opts.ignoreLocalSettings) {
    pnpmConfig.rootProjectManifest = await safeReadProjectManifestOnly(pnpmConfig.rootProjectManifestDir) ?? undefined
    if (pnpmConfig.rootProjectManifest != null) {
      if (pnpmConfig.rootProjectManifest.workspaces?.length && !pnpmConfig.workspaceDir) {
        warnings.push('The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead.')
      }
      const ignoredPnpmFieldKeys = getIgnoredPnpmFieldKeys(pnpmConfig.rootProjectManifest)
      if (ignoredPnpmFieldKeys.length > 0) {
        warnings.push(`The "pnpm" field in package.json is no longer read by pnpm. The following keys were ignored: ${quoteAndJoin(ignoredPnpmFieldKeys.map(k => `pnpm.${k}`))}. See https://pnpm.io/settings for the new home of each setting.`)
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
        const ignoredKeys = Object.keys(workspaceManifest).filter(isRefusedByAProjectManifest)
        if (ignoredKeys.length > 0) {
          warnings.push(`The following settings cannot be set in a project's pnpm-workspace.yaml and were ignored: ${quoteAndExplain(ignoredKeys)}.`)
        }
        addSettingsFromWorkspaceManifestToConfig(pnpmConfig, {
          configFromCliOpts,
          projectManifest: pnpmConfig.rootProjectManifest,
          skipSettings: opts.forSelfUpdate
            ? new Set([...PROJECT_MANIFEST_SKIPPED_KEYS, ...SELF_UPDATE_SKIPPED_SETTINGS])
            : PROJECT_MANIFEST_SKIPPED_KEYS,
          workspaceDir: pnpmConfig.workspaceDir,
          workspaceManifest,
        })
        if (workspaceManifest.registries != null) {
          workspaceManifestRegistries = pnpmConfig.registriesByScope as Record<string, string> | undefined
        }
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
        if (workspaceManifest.registries != null) {
          workspaceManifestRegistries = pnpmConfig.registriesByScope as Record<string, string> | undefined
        }
      }
    }
  }

  // Precedence: builtin < .npmrc < yaml < `_auth` < CLI. CLI
  // `--@scope:registry` / `--registry` already entered `registriesFromNpmrc`
  // via `authConfig`, so they're re-applied last here to avoid being buried
  // by yaml. `cliScopedRegistries` iterates raw `cliOptions` because
  // `explicitlySetKeys` is camelCased, which mangles `@org-a:registry`.
  pnpmConfig.registriesByScope = {
    ...registriesFromNpmrc,
    ...globalYamlRegistries,
    ...workspaceManifestRegistries,
    // `_auth` routes win over repo-controlled yaml on conflicting scopes.
    ...npmrcResult.jsonAuth.registries,
    // CLI per-scope registries last, so `--@scope:registry=...` wins over
    // both yaml and `_auth` ("CLI > _auth > yaml").
    ...cliScopedRegistries,
  }
  // Re-apply an unscoped `--registry` CLI flag last for the same reason
  // as `cliScopedRegistries` — it entered `registriesFromNpmrc` via
  // `authConfig.registry` and would otherwise be buried by env JSON.
  if (explicitlySetKeys.has('registry') && typeof pnpmConfig.registry === 'string') {
    pnpmConfig.registriesByScope.default = normalizeRegistryUrl(pnpmConfig.registry)
  }
  if (!pnpmConfig.registriesByScope.default) {
    pnpmConfig.registriesByScope.default = registriesFromNpmrc.default
  }
  for (const [scope, url] of Object.entries(pnpmConfig.registriesByScope)) {
    if (typeof url === 'string') {
      pnpmConfig.registriesByScope[scope] = normalizeRegistryUrl(url)
    }
  }

  // Sync registries.default to the top-level registry property so that
  // commands like login/logout that use opts.registry pick up the default
  // registry configured in pnpm-workspace.yaml. Only sync when the workspace
  // manifest actually contributed a different default than what .npmrc provided,
  // and when registry was not explicitly set via CLI.
  if (!explicitlySetKeys.has('registry') && pnpmConfig.registriesByScope.default !== registriesFromNpmrc.default) {
    pnpmConfig.registry = pnpmConfig.registriesByScope.default
  }

  // omit some schema that the custom parser can't yet handle
  const envPnpmTypes = omit([
    'init-version', // the type is a private function named 'semver'
    'node-version', // the type is a private function named 'semver'
    'umask', // the type is a private function named 'Umask'
  ], types)

  let virtualStoreTypeFromEnv: VirtualStoreType | undefined
  for (const { key, value } of parseEnvVars(key => envPnpmTypes[key as keyof typeof envPnpmTypes], env)) {
    // undefined means that the env key was defined, but its value couldn't be parsed according to the schema
    // TODO: should we throw some error or print some warning here?
    if (value === undefined) continue

    if (Object.hasOwn(cliOptions, key) || Object.hasOwn(cliOptions, kebabCase(key))) continue

    // Held back rather than assigned: the rest of pnpm reads the boolean
    // spelling, and applying the translation after the loop is what makes
    // the canonical key win over `PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE`
    // whichever order the two arrive in.
    if (key === 'virtualStoreType') {
      virtualStoreTypeFromEnv = value as VirtualStoreType
      continue
    }

    // @ts-expect-error
    pnpmConfig[key] = value
    explicitlySetKeys.add(key)

    if (key === 'registry') {
      if (typeof value !== 'string') {
        throw new TypeError(`Unexpected type of registry, expecting a string but received ${JSON.stringify(value)}`)
      }
      pnpmConfig.registriesByScope.default = normalizeRegistryUrl(value)
      pnpmConfig.packageManagerRegistries.default = normalizeRegistryUrl(value)
    }
  }
  if (virtualStoreTypeFromEnv != null) {
    pnpmConfig.enableGlobalVirtualStore = virtualStoreTypeFromEnv === 'global'
    explicitlySetKeys.add('enableGlobalVirtualStore')
  }

  // After the env loop: PNPM_CONFIG_REGISTRY can still change
  // `registries.default` above, and an entry matching it must not be reported
  // as unused.
  warnAboutUnmatchedRegistryOptions(pnpmConfig, warnings)

  // When the user explicitly sets `minimumReleaseAge`, treat it as strict by
  // default. Without this, a user-set value would silently fall back to
  // installing an immature version when no mature version satisfies the
  // requested range — making the setting look like it had no effect.
  // The built-in default for `minimumReleaseAge` is intentionally non-strict
  // for backward compatibility. This must run after env var parsing so
  // pnpm_config_minimum_release_age also enables strict mode.
  if (
    pnpmConfig.explicitlySetKeys.has('minimumReleaseAge') &&
    pnpmConfig.minimumReleaseAgeStrict == null
  ) {
    pnpmConfig.minimumReleaseAgeStrict = true
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
  // Default allowBuilds to {} when GVS is enabled and no build policy is
  // configured. This makes GVS hashes engine-agnostic for pure-JS packages.
  // When a build policy (dangerouslyAllowAllBuilds from global config.yaml,
  // or allowBuilds from the workspace manifest) exists, GVS hashes must
  // include ENGINE_NAME so that built packages and their dependents are
  // correctly invalidated across Node upgrades and architecture changes.
  if (
    pnpmConfig.enableGlobalVirtualStore &&
    pnpmConfig.allowBuilds == null &&
    pnpmConfig.dangerouslyAllowAllBuilds !== true
  ) {
    pnpmConfig.allowBuilds = {}
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
  // `catalogPrune`'s former name, still accepted. The canonical key wins
  // when both are set.
  pnpmConfig.catalogPrune ??= pnpmConfig.cleanupUnusedCatalogs
  // Every layer folds `virtualStoreType` into the boolean the rest of pnpm
  // reads, so the canonical spelling is restored here from the folded value
  // rather than from any one layer — otherwise `pnpm config get
  // virtualStoreType` could name the store a later layer overrode.
  if (explicitlySetKeys.has('enableGlobalVirtualStore')) {
    pnpmConfig.virtualStoreType = pnpmConfig.enableGlobalVirtualStore ? 'global' : 'project'
    explicitlySetKeys.add('virtualStoreType')
  }
  if (!pnpmConfig.httpsProxy) {
    // An empty `proxy=` is unset, so it must not suppress the environment
    // fallback. `false` and `null` keep their meaning: proxying is off.
    const legacyProxy = pnpmConfig.proxy === '' ? undefined : pnpmConfig.proxy
    pnpmConfig.httpsProxy = legacyProxy ?? getProcessEnv('https_proxy')
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

  // With a global virtual store, package directories live outside the
  // project, so Node's upward node_modules walk from their real paths never
  // reaches the project's hoisted node_modules or root node_modules. Expose
  // both through NODE_PATH for every child process pnpm spawns, and register
  // the ESM loader that restores NODE_PATH lookups for ESM imports.
  if (
    pnpmConfig.enableGlobalVirtualStore &&
    pnpmConfig.extendNodePath !== false &&
    (pnpmConfig.nodeLinker == null || pnpmConfig.nodeLinker === 'isolated')
  ) {
    const modulesDir = pathAbsolute(pnpmConfig.modulesDir ?? 'node_modules', pnpmConfig.rootProjectManifestDir)
    const nodePaths = [
      ...(pnpmConfig.extraEnv['NODE_PATH']?.split(path.delimiter) ?? []),
      path.join(modulesDir, '.pnpm', 'node_modules'),
      modulesDir,
    ]
    pnpmConfig.extraEnv['NODE_PATH'] = Array.from(new Set(nodePaths)).join(path.delimiter)
    pnpmConfig.extraEnv['NODE_OPTIONS'] = addEsmNodePathLoaderOption(env['NODE_OPTIONS'])
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
  // a specific behavior without editing the manifest. Otherwise, both the
  // legacy `packageManager` field and singular `devEngines.packageManager`
  // fall through to `download` (the documented default for `pmOnFail`); the
  // array form of `devEngines.packageManager` already has its own per-element
  // defaults applied during parsing.
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
    allProjects, selectedProjectsGraph, allProjectsGraph, prodAllProjectsGraph, prodOnlySelectedProjectDirs,
    rootProjectManifest, rootProjectManifestDir,
    cliOptions: ctxCliOptions,
    explicitlySetKeys: ctxExplicitlySetKeys,
    packageManager: ctxPackageManager, wantedPackageManager,
    ...config
  } = pnpmConfig as Config & ConfigContext
  const context: ConfigContext = {
    hooks, finders,
    allProjects, selectedProjectsGraph, allProjectsGraph, prodAllProjectsGraph, prodOnlySelectedProjectDirs,
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

function createPackageManagerNetworkConfig (
  trustedConfig: Record<string, unknown>,
  configByUri: PackageManagerNetworkConfig['configByUri'],
  env: Record<string, string | undefined>
): PackageManagerNetworkConfig {
  const httpsProxy = getProxyValue(
    trustedConfig['https-proxy'] ?? trustedConfig.proxy,
    getEnvValue(env, 'https_proxy')
  )
  const httpProxy = getProxyValue(
    trustedConfig['http-proxy'],
    httpsProxy ?? getEnvValue(env, 'http_proxy') ?? getEnvValue(env, 'proxy')
  )
  return {
    ca: trustedConfig.ca as string | string[] | undefined,
    cert: trustedConfig.cert as string | string[] | undefined,
    configByUri,
    httpProxy,
    httpsProxy,
    key: trustedConfig.key as string | undefined,
    localAddress: trustedConfig['local-address'] as string | undefined,
    noProxy: (trustedConfig['no-proxy'] ?? trustedConfig.noproxy ?? getEnvValue(env, 'no_proxy')) as string | boolean | undefined,
    strictSsl: trustedConfig['strict-ssl'] as boolean | undefined,
  }
}

function getEnvValue (env: Record<string, string | undefined>, key: string): string | undefined {
  return env[key] ?? env[key.toUpperCase()] ?? env[key.toLowerCase()]
}

function getProxyValue (value: unknown, fallback: string | undefined): string | undefined {
  if (value === false || value === null) return undefined
  if (typeof value === 'string' && value.length > 0) return value
  return fallback
}

// Look up a `pnpm_config_<key>` env var, accepting both lowercase and
// uppercase forms. Used for env vars that need to be read before the
// general parseEnvVars pass, such as those that affect which .npmrc file
// is loaded.
function readEnvVar (env: NodeJS.ProcessEnv, key: string): string | undefined {
  const value = env[`pnpm_config_${key}`] ?? env[`PNPM_CONFIG_${key.toUpperCase()}`]
  return value !== '' ? value : undefined
}

// Same shape as readEnvVar but for the `npm_config_<key>` family. Used as a
// low-priority compatibility shim so that npm-style env vars (e.g.
// NPM_CONFIG_USERCONFIG written by actions/setup-node) keep working.
function readNpmEnvVar (env: NodeJS.ProcessEnv, key: string): string | undefined {
  const value = env[`npm_config_${key}`] ?? env[`NPM_CONFIG_${key.toUpperCase()}`]
  return value !== '' ? value : undefined
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
      const conflictWarning = getPackageManagerConflictWarning(legacyPm, {
        name: pmFromDevEngines.name,
        ...splitPackageManagerVersion(pmFromDevEngines.version),
      })
      if (conflictWarning) {
        warnings.push(conflictWarning)
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

// Settings that pnpm reads from `pnpm-workspace.yaml` and never from the `pnpm`
// field of `package.json` — either because they moved there in v11, or because
// (like `update`) they were introduced later and only ever lived there. When one
// of these appears in the `pnpm` field, pnpm warns that it is ignored. Keys not
// in this set (e.g. `app`, or anything set by third-party tooling that piggybacks
// on the `pnpm` namespace) are left alone to avoid false-positive warnings.
const MIGRATED_PNPM_FIELD_KEYS = new Set<string>([
  'allowBuilds',
  'allowedDeprecatedVersions',
  'allowUnusedPatches',
  'audit',
  'auditConfig',
  'configDependencies',
  'executionEnv',
  'ignoredOptionalDependencies',
  'neverBuiltDependencies',
  'onlyBuiltDependencies',
  'onlyBuiltDependenciesFile',
  'overrides',
  'packageExtensions',
  'patchedDependencies',
  'peerDependencyRules',
  'requiredScripts',
  'supportedArchitectures',
  'update',
  'updateConfig',
])

function getIgnoredPnpmFieldKeys (manifest: ProjectManifest): string[] {
  const legacyField = (manifest as { pnpm?: unknown }).pnpm
  if (legacyField == null || typeof legacyField !== 'object' || Array.isArray(legacyField)) {
    return []
  }
  return Object.keys(legacyField as Record<string, unknown>).filter(k => MIGRATED_PNPM_FIELD_KEYS.has(k))
}

export interface ParsedPackageManager {
  name: string
  version: string | undefined
  hash: string | undefined
}

export function parsePackageManager (packageManager: string): ParsedPackageManager {
  // Split on the `@` that separates the name from the reference. A leading `@`
  // belongs to a scoped name (e.g. `@scope/pm@1.2.3`), so skip it; otherwise
  // the first `@` is the separator. The first `@` (not the last) is used so a
  // reference that is a URL containing `@` (e.g. credentials) stays intact.
  const separatorIndex = packageManager.startsWith('@')
    ? packageManager.indexOf('@', 1)
    : packageManager.indexOf('@')
  if (separatorIndex === -1) return { name: packageManager, version: undefined, hash: undefined }
  const name = packageManager.slice(0, separatorIndex)
  const pmReference = packageManager.slice(separatorIndex + 1)
  // pmReference is semantic versioning, not URL
  if (pmReference.includes(':')) return { name, version: undefined, hash: undefined }
  return { name, ...splitPackageManagerVersion(pmReference) }
}

/**
 * Splits a package manager version reference into its semver part and the
 * integrity hash carried as semver build metadata, e.g.
 * "9.5.0+sha512.140036830124618d624a2187b50d04289d5a087f326c9edfc0ccd733d76c4f52c3a313d4fc148794a2a9d81553016004e6742e8cf850670268a7387fc220c903"
 * becomes `{ version: "9.5.0", hash: "sha512.14003…" }`. A reference without a
 * hash yields an undefined hash; an undefined reference yields both undefined.
 */
function splitPackageManagerVersion (reference: string | undefined): { version: string | undefined, hash: string | undefined } {
  if (reference == null) return { version: undefined, hash: undefined }
  // Split on the first `+` only. The integrity hash is semver build metadata —
  // everything after that `+` — and must be preserved whole, so a reference is
  // never truncated at a later `+`.
  const hashIndex = reference.indexOf('+')
  if (hashIndex === -1) return { version: reference, hash: undefined }
  return { version: reference.slice(0, hashIndex), hash: reference.slice(hashIndex + 1) }
}

/**
 * Describes how the legacy `packageManager` field disagrees with
 * `devEngines.packageManager`, or returns undefined when the two specifiers are
 * identical (so keeping both fields in sync produces no warning). Any
 * divergence warns — including an integrity hash (semver build metadata) on
 * only one side, since dropping the ignored `packageManager` field would lose
 * it. In every conflict `devEngines.packageManager` wins and `packageManager`
 * is ignored.
 */
function getPackageManagerConflictWarning (legacy: ParsedPackageManager, devEngines: ParsedPackageManager): string | undefined {
  const ignoredSuffix = '. "packageManager" will be ignored'
  const genericWarning = `Cannot use both "packageManager" and "devEngines.packageManager" in package.json${ignoredSuffix}`
  if (legacy.name !== devEngines.name) {
    return `"packageManager" (${sanitizeManifestValue(legacy.name)}) and "devEngines.packageManager" (${sanitizeManifestValue(devEngines.name)}) specify different package managers in package.json${ignoredSuffix}`
  }
  if (legacy.version !== devEngines.version) {
    // "different versions" only makes sense when both sides are concrete
    // versions. If one side has no semver version — e.g. the legacy field is a
    // URL or a bare name — fall back to the generic notice rather than claiming
    // a version mismatch.
    if (legacy.version == null || devEngines.version == null) return genericWarning
    return `"packageManager" and "devEngines.packageManager" specify different versions of ${sanitizeManifestValue(legacy.name)} in package.json${ignoredSuffix}`
  }
  if (legacy.hash !== devEngines.hash) {
    // Same name and version, but the integrity hashes differ. Two distinct
    // hashes for one version is a likely wrong-hash mistake, so call it out
    // specifically; a hash on only one side is a softer mismatch (the version
    // still agrees) and gets the generic notice.
    if (legacy.hash != null && devEngines.hash != null && legacy.version != null) {
      return `"packageManager" and "devEngines.packageManager" specify ${sanitizeManifestValue(legacy.name)}@${sanitizeManifestValue(legacy.version)} with different integrity hashes in package.json${ignoredSuffix}`
    }
    return genericWarning
  }
  return undefined
}

/**
 * Renders a package.json-controlled value safe to embed in a warning printed to
 * the terminal. Strips ANSI escape sequences and replaces remaining control
 * characters (including newlines) with spaces so a malicious manifest cannot
 * forge or rewrite terminal/CI log output.
 */
function sanitizeManifestValue (value: string): string {
  // eslint-disable-next-line no-control-regex
  return stripVTControlCharacters(value).replace(/[\u0000-\u001f\u007f]/g, ' ')
}

/**
 * Decides whether the resolved pnpm integrity info should be written to
 * `pnpm-lock.yaml` under the project's `packageManagerDependencies` section.
 *
 * `onFail: ignore` means pnpm should not enforce or record the package manager
 * policy. Otherwise, `devEngines.packageManager` persists because it may use
 * ranges, while the legacy `packageManager` field only persists for pnpm v12+.
 */
export function shouldPersistLockfile (pm: Pick<WantedPackageManager, 'version' | 'fromDevEngines' | 'onFail'>): boolean {
  if (pm.onFail === 'ignore') return false
  if (pm.fromDevEngines === true) return true
  if (pm.version == null || semver.valid(pm.version) == null) return false
  return semver.major(pm.version) >= 12
}

function parseDevEnginesPackageManager (devEngines?: DevEngines): EngineDependency | undefined {
  if (!devEngines?.packageManager) return undefined
  let pmEngine: EngineDependency | undefined
  let onFail: 'ignore' | 'warn' | 'error' | 'download' | undefined
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
    // Singular form: leave onFail undefined when the user did not set it, so
    // the central pmOnFail default ('download') applies. The array form keeps
    // its own per-element defaults ('error' for the last entry, 'ignore' for
    // the rest) because those reflect explicit prioritization by the user.
    onFail = pmEngine.onFail
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
    if (!semver.validRange(nodeRuntime.version)) continue
    const minVersion = semver.minVersion(nodeRuntime.version)
    if (minVersion != null) {
      return minVersion.version
    }
  }
  return undefined
}

/**
 * Settings the project `pnpm-workspace.yaml` does not contribute to
 * `self-update`'s config.
 *
 * `self-update` replaces the pnpm binary every later install runs through, so
 * a repository must not get a say in whether it may be replaced. Each of these
 * is dangerous in both directions: a release-age cooldown lowered waives the
 * protection the user configured, raised it pins the machine to the installed
 * pnpm — including past a release that fixes a vulnerability in it; a
 * `trustPolicy` turned off accepts a pnpm release whose trust evidence the
 * user meant to reject, turned on blocks the update the same way; and `ci`
 * decides whether an immature pick may be confirmed at the keyboard at all.
 * Unlike a blocked dependency upgrade, those decisions follow the user out of
 * the repository. The policy therefore comes from the built-in defaults, the
 * global config yaml, the environment, and CLI flags only.
 */
const SELF_UPDATE_SKIPPED_SETTINGS = [
  'ci',
  'minimumReleaseAge',
  'minimumReleaseAgeExclude',
  'minimumReleaseAgeIgnoreMissingTime',
  'minimumReleaseAgeStrict',
  'trustPolicy',
  'trustPolicyExclude',
  'trustPolicyIgnoreAfter',
] as const satisfies ReadonlyArray<keyof Config>

/**
 * Where the machine keeps what it holds across runs, which no project chooses.
 *
 * A repository setting one would redirect where pnpm writes: `pnpm login`'s
 * `auth.ini`, `pnpm setup`'s PATH entry, the bins `pnpm install` links.
 */
const MACHINE_LOCATION_KEYS = [
  'configDir',
  'globalBinDir',
  'globalDir',
  'globalPkgDir',
  'npmrcAuthFile',
  'pnpmHomeDir',
  'stateDir',
  'userconfig',
] as const satisfies ReadonlyArray<keyof (Config & ConfigContext)>

/**
 * The directories the current command reads and writes in.
 *
 * The reader resolves these from the command line and the cwd, and it needs
 * them before it can find a manifest at all, so a manifest cannot supply them.
 */
const CURRENT_RUN_LOCATION_KEYS = [
  'bin',
  'dir',
  'rootProjectManifestDir',
  'workspaceDir',
] as const satisfies ReadonlyArray<keyof (Config & ConfigContext)>

/**
 * Which credentials pnpm sends, and to whom.
 *
 * The reader assembles these from the trusted config sources. `userConfig` is
 * the parsed contents of the user's `.npmrc`, so it carries credentials rather
 * than the path `npmrcAuthFile` holds.
 */
const CREDENTIAL_KEYS = [
  'authConfig',
  'userConfig',
  'configByUri',
  'packageManagerNetworkConfig',
  'packageManagerRegistries',
] as const satisfies ReadonlyArray<keyof (Config & ConfigContext)>

/**
 * Keys a project's `pnpm-workspace.yaml` does not contribute.
 *
 * `cacheDir` and `storeDir` are deliberately absent: those name caches a
 * project may legitimately place.
 */
type ProjectManifestSkippedKey =
  | typeof MACHINE_LOCATION_KEYS[number]
  | typeof CURRENT_RUN_LOCATION_KEYS[number]
  | typeof CREDENTIAL_KEYS[number]

/** Every key a caller of {@link addSettingsFromWorkspaceManifestToConfig} may skip. */
type SkippableKey =
  | ProjectManifestSkippedKey
  | typeof SELF_UPDATE_SKIPPED_SETTINGS[number]
  | typeof GLOBAL_CONFIG_ONLY_SKIPPED_KEYS[number]

const PROJECT_MANIFEST_SKIPPED_KEYS: ReadonlySet<ProjectManifestSkippedKey> = new Set([
  ...MACHINE_LOCATION_KEYS,
  ...CURRENT_RUN_LOCATION_KEYS,
  ...CREDENTIAL_KEYS,
])

/**
 * The refused keys the global config file does not accept either.
 *
 * That file's own contents are already filtered by {@link isConfigFileKey},
 * but the CLI options are merged in again alongside them, so without this a
 * `--config.config-dir` would land back on a key the reader resolves for
 * itself, and only for the users who happen to have a `config.yaml`.
 */
/**
 * Only the layout half of a `registries` entry is workspace-only: it decides
 * which tarball URLs are omitted from the lockfile, so a machine-local setting
 * would make one developer write a lockfile their collaborators read back with
 * a different layout. The routes to the registry are a legitimate global
 * preference, which is why the whole `registries` key is not refused here.
 */
const GLOBAL_CONFIG_ONLY_SKIPPED_KEYS = ['registryOptionsByUrl'] as const satisfies ReadonlyArray<keyof Config>

const GLOBAL_CONFIG_SKIPPED_KEYS: ReadonlySet<SkippableKey> = new Set([
  ...[...PROJECT_MANIFEST_SKIPPED_KEYS].filter((key) => !isConfigFileKey(kebabCase(key))),
  ...GLOBAL_CONFIG_ONLY_SKIPPED_KEYS,
])


/**
 * Whether a project's `pnpm-workspace.yaml` would ignore this camelCase key.
 * See {@link PROJECT_MANIFEST_SKIPPED_KEYS}.
 */
export function isProjectManifestSkippedKey (camelKey: string): boolean {
  const keys: ReadonlySet<string> = PROJECT_MANIFEST_SKIPPED_KEYS
  return keys.has(camelKey)
}

/**
 * Whether a project's `pnpm-workspace.yaml` drops {@link key}, given in either
 * camelCase or kebab-case, whether as a value a project may not contribute or
 * as the reader's own bookkeeping.
 *
 * Shared by the warnings so that they cannot disagree on what was dropped.
 */
function isRefusedByAProjectManifest (key: string): boolean {
  const camelKey = camelcase(key, { locale: 'en-US' })
  return isProjectManifestSkippedKey(camelKey) || CONFIG_CONTEXT_KEY_SET.has(camelKey)
}

/**
 * The reader's own bookkeeping, which shares one object with the settings but
 * is not settable by anyone.
 *
 * A manifest naming one of these would not choose a setting: it would
 * overwrite what the reader worked out, with a value of the wrong type.
 */
const CONFIG_CONTEXT_KEYS = [
  'hooks',
  'finders',
  'allProjects',
  'selectedProjectsGraph',
  'allProjectsGraph',
  'prodAllProjectsGraph',
  'prodOnlySelectedProjectDirs',
  'rootProjectManifest',
  'rootProjectManifestDir',
  'cliOptions',
  'explicitlySetKeys',
  'packageManager',
  'wantedPackageManager',
] as const satisfies ReadonlyArray<keyof ConfigContext>

type ProofConfigContextKeysIsExhaustive =
  (_: Record<typeof CONFIG_CONTEXT_KEYS[number], unknown>) => Record<keyof ConfigContext, unknown>

const _proofConfigContextKeysIsExhaustive: ProofConfigContextKeysIsExhaustive = (x) => x

const CONFIG_CONTEXT_KEY_SET: ReadonlySet<string> = new Set(CONFIG_CONTEXT_KEYS)

/**
 * The global config file key that sets a refused setting, where it is not that
 * setting's own name.
 */
const GLOBAL_EQUIVALENT_KEYS: Record<string, string> = {
  /** Derived from the global bin directory. */
  bin: 'global-bin-dir',
  /** Derived from the global directory. */
  globalPkgDir: 'global-dir',
  /**
   * Accepted under its own name, but never read back: the user-level `.npmrc`
   * comes from `npmrcAuthFile` or `--userconfig`, so its own name would send
   * the user to a command that changes nothing.
   */
  userconfig: 'npmrc-auth-file',
}

/**
 * Where {@link camelKey} can be set, for a key a project manifest refuses.
 *
 * Lives here rather than in the config command so that the reader's warnings
 * and the command's errors cannot drift into naming different routes for the
 * same setting.
 */
export function whereRefusedKeyBelongs (camelKey: string): string {
  if (camelKey === 'dir') return 'Pass --dir on the command line instead'
  const kebabKey = GLOBAL_EQUIVALENT_KEYS[camelKey] ?? kebabCase(camelKey)
  if (isConfigFileKey(kebabKey)) {
    return `Set it for the machine instead: pnpm config set --global ${kebabKey}`
  }
  return 'This is not a pnpm setting'
}

function quoteRefusedKey (key: string): string {
  return `"${key}" (${whereRefusedKeyBelongs(camelcase(key, { locale: 'en-US' }))})`
}

function quoteAndExplain (keys: string[]): string {
  return keys.map(quoteRefusedKey).join(', ')
}

function addSettingsFromWorkspaceManifestToConfig (pnpmConfig: Config & ConfigContext, {
  configFromCliOpts,
  expandRequestDestinationEnv,
  projectManifest,
  skipSettings,
  workspaceManifest,
  workspaceDir,
}: {
  configFromCliOpts: Record<string, unknown>
  expandRequestDestinationEnv?: boolean
  projectManifest: ProjectManifest | undefined
  /** Settings this manifest may not contribute, chosen by the caller. */
  skipSettings?: ReadonlySet<SkippableKey>
  workspaceDir: string | undefined
  workspaceManifest: WorkspaceManifest
}): void {
  const skipped: ReadonlySet<string> | undefined = skipSettings
  const newSettings = Object.assign(getOptionsFromPnpmSettings(workspaceDir, workspaceManifest, { manifest: projectManifest, expandRequestDestinationEnv }), configFromCliOpts)
  for (const [key, value] of Object.entries(newSettings)) {
    if (!isCamelCase(key)) continue
    if (CONFIG_CONTEXT_KEY_SET.has(key)) continue
    if (skipped?.has(key)) continue

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

/**
 * A `registries` entry that routes nothing to itself — no `scopes`, no
 * `prefix` — describes a registry configured elsewhere, so it only takes
 * effect when its key is one pnpm actually resolves from. A key that matches
 * none is inert: the wrong URL, a stale entry, a scope that moved. Warn rather
 * than throw, because a shared config dependency can legitimately describe
 * registries a given project does not use.
 */
function warnAboutUnmatchedRegistryOptions (config: Config, warnings: string[]): void {
  const registryOptionsByUrl = config.registryOptionsByUrl
  if (registryOptionsByUrl == null) return
  const configuredRegistries = new Set([
    ...Object.values(config.registriesByScope),
    ...Object.values(config.registriesByPrefix ?? {}),
    ...Object.values(BUILTIN_REGISTRIES_BY_PREFIX),
  ].map(normalizeRegistryUrl))
  const unmatched = Object.keys(registryOptionsByUrl).filter((registry) => !configuredRegistries.has(registry))
  if (unmatched.length === 0) return
  // A registry URL can carry `user:pass@` credentials, and the global config's
  // keys have had `${VAR}` expanded by this point, so neither list may be
  // echoed raw into a terminal or a CI log.
  warnings.push(
    `The following "registries" entries do not match any configured registry and were ignored: ${quoteAndJoin(unmatched.map(redactAndSanitize))}. ` +
    `The configured registries are: ${quoteAndJoin([...configuredRegistries].sort().map(redactAndSanitize))}.`
  )
}
