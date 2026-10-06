import { PnpmError, redactAndSanitize } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import {
  DEFAULT_REGISTRY_SCOPE,
  type PnpmSettings,
  type RegistryDeclaration,
  type RegistryOptions,
} from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'

import type { OptionsFromRootManifest } from './getOptionsFromRootManifest.js'
import { quoteAndJoin } from './quoteAndJoin.js'
import { redactRegistryUrl, registryUrlHasUserinfo } from './registryUrlUserinfo.js'
import { assertBoolean, assertObjectSetting, assertString, assertStringArray } from './settingAssertions.js'

const REGISTRY_SERVER_TYPES = new Set(['npm', 'artifactory'])

/**
 * Credentials and TLS material stay in `.npmrc`, which is not committed.
 * `registries` lives in `pnpm-workspace.yaml`, which is, so accepting these
 * here would invite secrets into version control. Rejecting is better than
 * ignoring: a silently dropped `_authToken` reads as configured.
 */
const SECRET_REGISTRY_KEYS = new Set([
  '_auth', '_authToken', '_password', 'username', 'tokenHelper',
  'ca', 'cafile', 'cert', 'certfile', 'key', 'keyfile',
])

/** The fields a `registries` entry may carry. Anything else is a typo. */
const REGISTRY_DECLARATION_FIELDS = new Set(['serverType', 'supportsTimeField', 'scopes', 'prefix'])

/**
 * Turns the settings that name registries into the lookups the rest of
 * pnpm reads: the scope-routed URLs, the `<prefix>:`-addressed URLs, and the
 * per-registry options. The declaration map itself does not travel.
 */
export function translateRegistrySettings (settings: OptionsFromRootManifest): void {
  // Both settings are read under the names a user writes and removed, because
  // the keys the rest of pnpm reads are the lookups they feed.
  const written = settings as Pick<PnpmSettings, 'namedRegistries' | 'registries'>
  const deprecatedPrefixes = written.namedRegistries
  delete written.namedRegistries
  const declarations = written.registries
  delete written.registries

  const lookups = readRegistryDeclarations(declarations)
  if (deprecatedPrefixes != null && Object.keys(lookups.registriesByPrefix).length > 0) {
    globalWarn('Both the "registries" and "namedRegistries" settings declare registry prefixes. The deprecated "namedRegistries" setting is only read for prefixes "registries" does not declare.')
  }
  // A prefix a `registries` entry declares wins: the two spell the same thing.
  const registriesByPrefix = { ...deprecatedPrefixes, ...lookups.registriesByPrefix }
  if (Object.keys(registriesByPrefix).length > 0) settings.registriesByPrefix = registriesByPrefix
  if (Object.keys(lookups.registriesByScope).length > 0) settings.registriesByScope = lookups.registriesByScope
  if (Object.keys(lookups.registryOptionsByUrl).length > 0) settings.registryOptionsByUrl = lookups.registryOptionsByUrl
}

/**
 * Reads the `registries` setting and inverts the routes each entry declares,
 * or returns `undefined` when there is nothing to invert.
 *
 * A registry is declared once, keyed by its URL, because every fact in an
 * entry is a fact about that server: how it lays out tarball URLs, and which
 * routes reach it. The lookups are the inverse of the routes, because a scope
 * resolves to exactly one registry while a registry serves many.
 *
 * The older shape — a map whose values are all strings — is the scope lookup
 * already, and is left in place as it stands.
 */
function readRegistryDeclarations (declarations: PnpmSettings['registries']): RegistryLookups {
  const empty: RegistryLookups = { registriesByScope: {}, registriesByPrefix: {}, registryOptionsByUrl: {} }
  if (declarations == null) return empty
  assertObjectSetting(declarations, 'registries')
  const entries = Object.entries(declarations)
  const scopeRoutes = entries.filter(([, declaration]) => typeof declaration === 'string')
  if (scopeRoutes.length === entries.length) {
    assertNoUrlKeyedScopeRoutes(scopeRoutes.map(([scope]) => scope))
    return { ...empty, registriesByScope: Object.fromEntries(scopeRoutes as Array<[string, string]>) }
  }
  if (scopeRoutes.length > 0) {
    throw new PnpmError('INVALID_SETTING',
      `The "registries" setting mixes registry declarations with "<scope>: <url>" entries (${quoteAndJoin(scopeRoutes.map(([scope]) => scope))}).`,
      { hint: 'Key every entry by registry URL and list the scopes routed to it under "scopes".' })
  }
  return splitRegistryDeclarations(entries as Array<[string, RegistryDeclaration]>)
}

/**
 * A scope routes to a registry, so a URL in that position routes nothing and
 * would sit there inert. It is the declaration shape, half-written.
 */
function assertNoUrlKeyedScopeRoutes (scopes: string[]): void {
  for (const scope of scopes) {
    if (!looksLikeRegistryUrl(scope)) continue
    throw new PnpmError('INVALID_SETTING',
      `The "registries['${redactRegistryUrl(scope)}']" entry is a string.`,
      { hint: 'A registry URL keys a declaration, e.g. { serverType: "artifactory" }. A string value routes a scope, and a URL is not a scope.' })
  }
}

interface RegistryLookups {
  /** Scope-routed URLs, normalized, with `@` under its `default` key. */
  registriesByScope: Record<string, string>
  /**
   * Prefix-addressed URLs, deliberately kept as written: a named registry's
   * URL is what a lockfile's recorded tarball URLs are matched against, so
   * normalizing it would change what an existing lockfile verifies against.
   */
  registriesByPrefix: Record<string, string>
  registryOptionsByUrl: Record<string, RegistryOptions>
}

/** Validates the declarations and inverts their routes into the lookups. */
function splitRegistryDeclarations (entries: Array<[string, RegistryDeclaration]>): RegistryLookups {
  const lookups: RegistryLookups = { registriesByScope: {}, registriesByPrefix: {}, registryOptionsByUrl: {} }
  for (const [registry, declaration] of entries) {
    addRegistryDeclaration(lookups, registry, declaration)
  }
  return lookups
}

interface RegistryDeclarationContext {
  registry: string
  /** The registry URL normalized for the lookups keyed by URL. */
  normalizedRegistry: string
  settingPath: string
}

function addRegistryDeclaration (lookups: RegistryLookups, registry: string, declaration: RegistryDeclaration): void {
  // The URL is user config that may carry `user:pass@` credentials, and it
  // is about to be interpolated into an error a terminal or CI log will show.
  const settingPath = `registries['${redactRegistryUrl(registry)}']`
  assertValidRegistryDeclarationShape(declaration, registry, settingPath)
  // Normalized for the lookups keyed by URL, so that a registry a package
  // resolved from matches its declaration however either one spelled the
  // trailing slash.
  const context: RegistryDeclarationContext = { registry, normalizedRegistry: normalizeRegistryUrl(registry), settingPath }
  const { scopes, prefix } = declaration
  addRegistryOptions(lookups, declaration, context)
  if (scopes != null) {
    addScopeRoutes(lookups, scopes, context)
  }
  if (prefix != null) {
    addPrefixRoute(lookups, prefix, context)
  }
}

function assertValidRegistryDeclarationShape (declaration: RegistryDeclaration, registry: string, settingPath: string): void {
  assertObjectSetting(declaration, settingPath)
  assertKnownRegistryFields(declaration, registry, settingPath)
  // The map lives in the committed pnpm-workspace.yaml, and it already
  // refuses credential fields for that reason; a credential in the key is
  // the same secret in the same file. A registry whose URL really carries
  // credentials should move them to .npmrc, which also makes the URL here
  // match the one pnpm resolves from.
  if (registryUrlHasUserinfo(registry)) {
    throw new PnpmError('INVALID_SETTING',
      `The "${settingPath}" key embeds credentials.`,
      { hint: 'Put them in an .npmrc file instead, so they are not committed.' })
  }
}

function addRegistryOptions (
  lookups: RegistryLookups,
  { serverType, supportsTimeField }: RegistryDeclaration,
  { normalizedRegistry, settingPath }: RegistryDeclarationContext
): void {
  if (serverType != null && !REGISTRY_SERVER_TYPES.has(serverType)) {
    throw new PnpmError('INVALID_SETTING',
      `The "${settingPath}.serverType" setting should be one of ${quoteAndJoin([...REGISTRY_SERVER_TYPES])}, but got ${JSON.stringify(serverType)}`)
  }
  if (supportsTimeField != null) {
    assertBoolean(supportsTimeField, `${settingPath}.supportsTimeField`)
  }
  if (serverType == null && supportsTimeField == null) return
  lookups.registryOptionsByUrl[normalizedRegistry] = {
    ...(serverType != null ? { serverType } : {}),
    ...(supportsTimeField != null ? { supportsTimeField } : {}),
  }
}

function addScopeRoutes (
  lookups: RegistryLookups,
  scopes: unknown,
  { normalizedRegistry, settingPath }: RegistryDeclarationContext
): void {
  assertStringArray(scopes, `${settingPath}.scopes`)
  for (const scope of scopes) {
    if (!scope.startsWith(DEFAULT_REGISTRY_SCOPE)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}.scopes" setting should list "@"-prefixed scopes, but got ${JSON.stringify(scope)}`,
        { hint: `A bare "${DEFAULT_REGISTRY_SCOPE}" is the scope-less default registry.` })
    }
    const scopeKey = scope === DEFAULT_REGISTRY_SCOPE ? 'default' : scope
    const routed = lookups.registriesByScope[scopeKey]
    if (routed != null && routed !== normalizedRegistry) {
      throw new PnpmError('INVALID_SETTING',
        `The scope ${JSON.stringify(scope)} is routed to two registries: ${quoteAndJoin([routed, normalizedRegistry].map(redactAndSanitize))}.`)
    }
    lookups.registriesByScope[scopeKey] = normalizedRegistry
  }
}

function addPrefixRoute (
  lookups: RegistryLookups,
  prefix: unknown,
  { registry, settingPath }: RegistryDeclarationContext
): void {
  assertString(prefix, `${settingPath}.prefix`)
  const declaredBy = Object.hasOwn(lookups.registriesByPrefix, prefix) ? lookups.registriesByPrefix[prefix] : undefined
  if (declaredBy != null) {
    throw new PnpmError('INVALID_SETTING',
      `The prefix ${JSON.stringify(prefix)} is declared by two registries: ${quoteAndJoin([declaredBy, registry].map(redactAndSanitize))}.`)
  }
  lookups.registriesByPrefix[prefix] = registry
}

/**
 * A misspelled field would otherwise sit there doing nothing, and a credential
 * field would sit in a committed file. Both are refused here rather than by
 * the yaml parser, which renders the offending source line verbatim under its
 * errors — printing the very credential being refused.
 */
function assertKnownRegistryFields (declaration: RegistryDeclaration, registry: string, settingPath: string): void {
  for (const field of Object.keys(declaration)) {
    if (REGISTRY_DECLARATION_FIELDS.has(field)) continue
    if (SECRET_REGISTRY_KEYS.has(field)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}.${field}" setting is not allowed in pnpm-workspace.yaml.`,
        { hint: `Set "//${redactRegistryUrl(registry).replace(/^(?:https?:)?\/\//, '')}:${field}" in an .npmrc file instead, so it is not committed.` })
    }
    throw new PnpmError('INVALID_SETTING',
      `The "${settingPath}.${field}" setting is not a known registry setting.`,
      { hint: `A registry declares ${quoteAndJoin([...REGISTRY_DECLARATION_FIELDS])}.` })
  }
}

function looksLikeRegistryUrl (key: string): boolean {
  return key.includes('://') || key.startsWith('//')
}
