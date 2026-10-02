import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'
import { redactAndSanitize } from '@pnpm/error'
import type { RegistriesByScope } from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

import type { Config, PackageManagerNetworkConfig } from '../Config.js'
import { getNetworkConfigs } from '../getNetworkConfigs.js'
import { pickIniConfig } from '../localConfig.js'
import { quoteAndJoin } from '../quoteAndJoin.js'
import { resolveJsonAuthRegistries } from '../resolveJsonAuthRegistries.js'
import type { ConfigBuildState } from './configBuildState.js'

export interface InitialRegistries {
  /** The builtin default plus what the `.npmrc` files and the command line declared. */
  registriesFromNpmrc: RegistriesByScope
  cliScopedRegistries: Record<string, string>
}

export function resolveInitialRegistries (state: ConfigBuildState): InitialRegistries {
  const { pnpmConfig } = state
  const networkConfigs = getNetworkConfigs(pnpmConfig.authConfig)
  const registriesFromNpmrc = {
    default: normalizeRegistryUrl(pnpmConfig.authConfig.registry),
    ...networkConfigs.registries,
  }
  const trustedAuthConfig = pickIniConfig(state.npmrcResult.trustedConfig)
  const trustedNetworkConfigs = getNetworkConfigs(trustedAuthConfig)
  const cliScopedRegistries = pickCliScopedRegistries(state.cliOptions)
  pnpmConfig.registriesByScope = { ...registriesFromNpmrc }
  const explicitDefaultRegistry = getExplicitlySetDefaultRegistry(state)
  if (explicitDefaultRegistry != null) {
    pnpmConfig.registriesByScope.default = explicitDefaultRegistry
  }
  const packageManagerRegistries = resolvePackageManagerRegistries(state, {
    cliScopedRegistries,
    trustedDefaultRegistry: trustedAuthConfig.registry as string,
    trustedRegistries: trustedNetworkConfigs.registries,
  })
  if (explicitDefaultRegistry != null) {
    packageManagerRegistries.default = explicitDefaultRegistry
  }
  pnpmConfig.packageManagerRegistries = packageManagerRegistries
  pnpmConfig.packageManagerNetworkConfig = createPackageManagerNetworkConfig(
    state.npmrcResult.trustedConfig,
    trustedNetworkConfigs.configByUri ?? {},
    state.env
  )
  pnpmConfig.configByUri = { ...networkConfigs.configByUri }
  return { registriesFromNpmrc, cliScopedRegistries }
}

function pickCliScopedRegistries (cliOptions: Record<string, unknown>): Record<string, string> {
  const cliScopedRegistries: Record<string, string> = {}
  for (const [key, value] of Object.entries(cliOptions)) {
    if (key.startsWith('@') && key.endsWith(':registry') && typeof value === 'string') {
      cliScopedRegistries[key.slice(0, -':registry'.length)] = normalizeRegistryUrl(value)
    }
  }
  return cliScopedRegistries
}

function getExplicitlySetDefaultRegistry ({ explicitlySetKeys, pnpmConfig }: ConfigBuildState): string | undefined {
  return explicitlySetKeys.has('registry') && typeof pnpmConfig.registry === 'string'
    ? normalizeRegistryUrl(pnpmConfig.registry)
    : undefined
}

function resolvePackageManagerRegistries (
  { npmrcResult }: ConfigBuildState,
  { cliScopedRegistries, trustedDefaultRegistry, trustedRegistries }: {
    cliScopedRegistries: Record<string, string>
    trustedDefaultRegistry: string
    trustedRegistries: Record<string, string>
  }
): RegistriesByScope {
  // Only the trusted `.npmrc` files reach the bootstrap cascade, so only what
  // they declare decides which `_auth` routes apply to it.
  const bootstrapJsonAuthRegistries = resolveJsonAuthRegistries(npmrcResult.jsonAuth, {
    registries: { ...trustedRegistries, ...npmrcResult.trustedDeclaredRegistries },
    defaultRegistry: npmrcResult.trustedDeclaredRegistries.default,
  })
  return {
    default: normalizeRegistryUrl(trustedDefaultRegistry),
    // The file fallback applies to the bootstrap cascade too, so a registry
    // reached only through a stored credential is reached the same way when
    // pnpm downloads itself as when it installs.
    ...bootstrapJsonAuthRegistries.fallbackRegistries,
    // A `registry=` in a trusted `.npmrc` declares the default registry as
    // plainly as a yaml does, so it holds the file fallback back here too.
    ...npmrcResult.trustedDeclaredRegistries,
    ...trustedRegistries,
    // `_auth` routes apply here too so bootstrap (self-download / version
    // switching) resolves the same way as regular installs.
    ...bootstrapJsonAuthRegistries.envRegistries,
    ...cliScopedRegistries,
  }
}

export interface RegistriesByScopeSources extends InitialRegistries {
  globalYamlRegistries: Record<string, string> | undefined
  workspaceManifestRegistries: Record<string, string> | undefined
}

/**
 * Precedence: builtin < `_auth` file < .npmrc < yaml < `_auth` env < CLI. CLI
 * `--@scope:registry` / `--registry` already entered `registriesFromNpmrc`
 * via `authConfig`, so they're re-applied last here to avoid being buried
 * by yaml. `cliScopedRegistries` iterates raw `cliOptions` because
 * `explicitlySetKeys` is camelCased, which mangles `@org-a:registry`.
 */
export function resolveRegistriesByScope (state: ConfigBuildState, sources: RegistriesByScopeSources): void {
  const { pnpmConfig } = state
  pnpmConfig.registriesByScope = mergeRegistriesByScope(state, sources)
  const { registriesFromNpmrc } = sources
  if (state.registrySetOnCommandLine && typeof pnpmConfig.registry === 'string') {
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
  if (!state.registrySetOnCommandLine && pnpmConfig.registriesByScope.default !== registriesFromNpmrc.default) {
    pnpmConfig.registry = pnpmConfig.registriesByScope.default
  }
}

function mergeRegistriesByScope (
  state: ConfigBuildState,
  { registriesFromNpmrc, cliScopedRegistries, globalYamlRegistries, workspaceManifestRegistries }: RegistriesByScopeSources
): RegistriesByScope {
  // A `registry:` in either yaml declares the default as plainly as a
  // `registries` entry does, but reaches `pnpmConfig.registry` instead of the
  // map, so it has to be restated here to outrank the `_auth` file fallback.
  // Asked of the key rather than of its value: pinning the registry a lower
  // layer already resolved to is still a declaration.
  const explicitDefaultRegistry = getExplicitlySetDefaultRegistry(state)
  const declaredDefault = explicitDefaultRegistry != null ? { default: explicitDefaultRegistry } : undefined
  const { npmrcResult } = state
  const declaredRegistries = {
    ...npmrcResult.declaredRegistries,
    ...globalYamlRegistries,
    ...workspaceManifestRegistries,
  }
  const jsonAuthRegistries = resolveJsonAuthRegistries(npmrcResult.jsonAuth, {
    registries: { ...registriesFromNpmrc, ...declaredRegistries },
    defaultRegistry: declaredDefault?.default ?? declaredRegistries.default,
  })
  return {
    ...registriesFromNpmrc,
    // The global config file's `_auth` only fills in what nothing declares:
    // it is where a `pnpm login` stores a credential, and holding one is not
    // a statement about where packages come from. `registriesFromNpmrc`
    // carries the builtin default as well as what the `.npmrc` files
    // declared, so only the latter are restated above the fallback.
    ...jsonAuthRegistries.fallbackRegistries,
    ...declaredRegistries,
    ...declaredDefault,
    // The `_auth` env var is the operator's channel — a CI runner pointed at
    // a mandated proxy — so its routes win over what any file declares.
    ...jsonAuthRegistries.envRegistries,
    ...cliScopedRegistries,
  }
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

/**
 * A `registries` entry that routes nothing to itself — no `scopes`, no
 * `prefix` — describes a registry configured elsewhere, so it only takes
 * effect when its key is one pnpm actually resolves from. A key that matches
 * none is inert: the wrong URL, a stale entry, a scope that moved. Warn rather
 * than throw, because a shared config dependency can legitimately describe
 * registries a given project does not use.
 */
export function warnAboutUnmatchedRegistryOptions (config: Config, warnings: string[]): void {
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
