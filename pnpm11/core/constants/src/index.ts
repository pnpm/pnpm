export const WANTED_LOCKFILE = 'pnpm-lock.yaml'
export const LOCKFILE_MAJOR_VERSION = '9'
export const LOCKFILE_VERSION = `${LOCKFILE_MAJOR_VERSION}.0`

export const MANIFEST_BASE_NAMES = ['package.json', 'package.json5', 'package.yaml'] as const

export const ENGINE_NAME = `${process.platform};${process.arch};node${process.version.split('.')[0].substring(1)}`
export const LAYOUT_VERSION = 5
export const STORE_VERSION = 'v11'
export const GLOBAL_LAYOUT_VERSION = 'v11'

export const GLOBAL_CONFIG_YAML_FILENAME = 'config.yaml'
export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

/**
 * Named-registry aliases that work without any configuration. User entries in
 * the `registriesByPrefix` setting are merged on top and may override these
 * (e.g. GHES users can point `gh` at their own enterprise host).
 *
 * `npmjs` is here so a dependency can be pinned to the public registry even
 * when `registry` points somewhere else, such as an internal proxy. The `npm`
 * prefix cannot serve that purpose: it is reserved for the alias protocol
 * (`npm:<name>@<range>`), which resolves through the default registry.
 *
 * Because these URLs are also the prefixes a recorded tarball URL is matched
 * against, an org that proxies npmjs should point `npmjs` at their proxy so
 * verification keeps going there rather than to the public host.
 */
export const BUILTIN_REGISTRIES_BY_PREFIX: Readonly<Record<string, string>> = Object.freeze({
  gh: 'https://npm.pkg.github.com/',
  npmjs: 'https://registry.npmjs.org/',
})

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
//
// The version prefix is bumped whenever the file format changes, so a pnpm
// version that predates the change never reads files it cannot parse (and
// never overwrites them with the format it knows). `v12` is the indexed
// layout in `resolving/npm-resolver`'s mirror module; `v11` was NDJSON.
export const ABBREVIATED_META_DIR = 'v12/metadata'
export const FULL_META_DIR = 'v12/metadata-full'
export const FULL_FILTERED_META_DIR = 'v12/metadata-full-filtered'
