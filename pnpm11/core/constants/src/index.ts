export const WANTED_LOCKFILE = 'pnpm-lock.yaml'
export const LOCKFILE_MAJOR_VERSION = '9'
export const LOCKFILE_VERSION = `${LOCKFILE_MAJOR_VERSION}.0`
/**
 * The lockfile format that records named-registry packages under
 * registry-qualified keys (`<name>@<registryName>:<version>`). Stamped only
 * when the lockfile actually contains such a package, so lockfiles that don't
 * use named registries stay on LOCKFILE_VERSION byte for byte.
 */
export const NAMED_REGISTRIES_LOCKFILE_VERSION = `${LOCKFILE_MAJOR_VERSION}.1`
export const SUPPORTED_LOCKFILE_VERSIONS = [LOCKFILE_VERSION, NAMED_REGISTRIES_LOCKFILE_VERSION]

export const MANIFEST_BASE_NAMES = ['package.json', 'package.json5', 'package.yaml'] as const

export const ENGINE_NAME = `${process.platform};${process.arch};node${process.version.split('.')[0].substring(1)}`
export const LAYOUT_VERSION = 5
export const STORE_VERSION = 'v11'
export const GLOBAL_LAYOUT_VERSION = 'v11'

export const GLOBAL_CONFIG_YAML_FILENAME = 'config.yaml'
export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

/**
 * Named-registry aliases that work without any configuration. User entries in
 * the `namedRegistries` setting are merged on top and may override these
 * (e.g. GHES users can point `gh` at their own enterprise host).
 */
export const BUILTIN_NAMED_REGISTRIES: Readonly<Record<string, string>> = Object.freeze({
  gh: 'https://npm.pkg.github.com/',
})

/**
 * The built-in named registries with the user's `namedRegistries` merged on
 * top, as a null-prototype record.
 *
 * The prototype matters: alias names come out of the lockfile's dep paths, so
 * a crafted `foo@constructor:1.0.0` would otherwise look up
 * `Object.prototype.constructor` — a truthy function — and sail past every
 * `if (!registry)` guard that is there to fail closed on an unknown alias.
 * Always resolve an alias through this, never through a plain object literal.
 */
export function resolveNamedRegistries (userDefined?: Record<string, string>): Record<string, string> {
  return Object.assign(Object.create(null) as Record<string, string>, BUILTIN_NAMED_REGISTRIES, userDefined)
}

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
//
// Cache files use NDJSON format: line 1 is cache headers (etag, modified),
// line 2 is the registry metadata JSON.
export const ABBREVIATED_META_DIR = 'v11/metadata'
export const FULL_META_DIR = 'v11/metadata-full'
export const FULL_FILTERED_META_DIR = 'v11/metadata-full-filtered'

