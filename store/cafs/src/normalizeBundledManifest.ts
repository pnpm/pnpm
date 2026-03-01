import { type BaseManifest, type BundledManifest } from '@pnpm/types'
import semver from 'semver'

const BUNDLED_MANIFEST_FIELDS: Array<keyof BaseManifest> = [
  'bin',
  'bundledDependencies',
  'bundleDependencies',
  'cpu',
  'dependencies',
  'devDependencies',
  'directories',
  'engines',
  'libc',
  'name',
  'optionalDependencies',
  'os',
  'peerDependencies',
  'peerDependenciesMeta',
]

const LIFECYCLE_SCRIPTS = ['preinstall', 'install', 'postinstall'] as const

/**
 * Picks the subset of manifest fields stored in the package index and normalizes the version.
 * Used both when writing the index (worker) and when creating a BundledManifest from a fresh fetch.
 */
export function normalizeBundledManifest (manifest: Partial<BaseManifest>): BundledManifest | undefined {
  let result: Record<string, unknown> | undefined
  for (const key of BUNDLED_MANIFEST_FIELDS) {
    if (manifest[key] != null) {
      if (!result) result = {}
      result[key] = manifest[key]
    }
  }
  let scripts: Record<string, string> | undefined
  if (manifest.scripts) {
    for (const key of LIFECYCLE_SCRIPTS) {
      if (manifest.scripts[key]) {
        if (!scripts) scripts = {}
        scripts[key] = manifest.scripts[key]
      }
    }
  }
  if (!result && !scripts) return undefined
  return {
    version: semver.clean(manifest.version ?? '0.0.0', { loose: true }) ?? manifest.version,
    ...result,
    ...scripts ? { scripts } : {},
  } as BundledManifest
}
