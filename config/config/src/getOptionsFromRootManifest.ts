import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type SupportedArchitectures,
  type AllowedDeprecatedVersions,
  type PackageExtension,
  type PeerDependencyRules,
  type ProjectManifest,
  type PnpmSettings,
} from '@pnpm/types'
import mapValues from 'ramda/src/map'
import pick from 'ramda/src/pick'

export type OptionsFromRootManifest = {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowNonAppliedPatches?: boolean
  overrides?: Record<string, string>
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  onlyBuiltDependenciesFile?: string
  ignoredBuiltDependencies?: string[]
  packageExtensions?: Record<string, PackageExtension>
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: Record<string, string>
  peerDependencyRules?: PeerDependencyRules
  supportedArchitectures?: SupportedArchitectures
} & Pick<PnpmSettings, 'configDependencies'>

export function getOptionsFromRootManifest (manifestDir: string, manifest: ProjectManifest): OptionsFromRootManifest {
  const settings: OptionsFromRootManifest = getOptionsFromPnpmSettings(manifestDir, {
    ...manifest.pnpm,
    // We read Yarn's resolutions field for compatibility
    // but we really replace the version specs to any other version spec, not only to exact versions,
    // so we cannot call it resolutions
    overrides: {
      ...manifest.resolutions,
      ...manifest.pnpm?.overrides,
    },
  }, manifest)
  return settings
}

export function getOptionsFromPnpmSettings (manifestDir: string, pnpmSettings: PnpmSettings, manifest?: ProjectManifest): OptionsFromRootManifest {
  const settings: OptionsFromRootManifest = pick([
    'allowNonAppliedPatches',
    'allowedDeprecatedVersions',
    'configDependencies',
    'ignoredBuiltDependencies',
    'ignoredOptionalDependencies',
    'neverBuiltDependencies',
    'onlyBuiltDependencies',
    'onlyBuiltDependenciesFile',
    'overrides',
    'packageExtensions',
    'peerDependencyRules',
    'supportedArchitectures',
  ], pnpmSettings)
  if (settings.overrides && manifest) {
    settings.overrides = mapValues(createVersionReferencesReplacer(manifest), settings.overrides)
  }
  if (pnpmSettings.onlyBuiltDependenciesFile) {
    settings.onlyBuiltDependenciesFile = path.join(manifestDir, pnpmSettings.onlyBuiltDependenciesFile)
  }
  if (pnpmSettings.patchedDependencies) {
    settings.patchedDependencies = { ...pnpmSettings.patchedDependencies }
    for (const [dep, patchFile] of Object.entries(pnpmSettings.patchedDependencies)) {
      if (path.isAbsolute(patchFile)) continue
      settings.patchedDependencies[dep] = path.join(manifestDir, patchFile)
    }
  }
  return settings
}

function createVersionReferencesReplacer (manifest: ProjectManifest): (spec: string) => string {
  const allDeps = {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
  return replaceVersionReferences.bind(null, allDeps)
}

function replaceVersionReferences (dep: Record<string, string>, spec: string): string {
  if (!(spec[0] === '$')) return spec
  const dependencyName = spec.slice(1)
  const newSpec = dep[dependencyName]
  if (newSpec) return newSpec
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${spec} in overrides. The direct dependencies don't have dependency "${dependencyName}".`
  )
}
