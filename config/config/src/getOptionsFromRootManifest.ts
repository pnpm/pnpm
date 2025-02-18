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
  // We read Yarn's resolutions field for compatibility
  // but we really replace the version specs to any other version spec, not only to exact versions,
  // so we cannot call it resolutions
  const overrides = mapValues(
    createVersionReferencesReplacer(manifest),
    {
      ...manifest.resolutions,
      ...manifest.pnpm?.overrides,
    }
  )

  const settings: OptionsFromRootManifest = {
    overrides,
    ...(manifest.pnpm ? getOptionsFromPnpmSettings(manifestDir, manifest.pnpm) : {}),
  }
  if (settings.neverBuiltDependencies == null && settings.onlyBuiltDependencies == null && settings.onlyBuiltDependenciesFile == null) {
    settings.onlyBuiltDependencies = []
  }
  return settings
}

export function getOptionsFromPnpmSettings (manifestDir: string, pnpmSettings: PnpmSettings): OptionsFromRootManifest {
  const neverBuiltDependencies = pnpmSettings.neverBuiltDependencies
  let onlyBuiltDependencies = pnpmSettings.onlyBuiltDependencies
  const onlyBuiltDependenciesFile = pnpmSettings.onlyBuiltDependenciesFile
  if (onlyBuiltDependenciesFile == null && neverBuiltDependencies == null && onlyBuiltDependencies == null) {
    onlyBuiltDependencies = []
  }
  const packageExtensions = pnpmSettings.packageExtensions
  const ignoredOptionalDependencies = pnpmSettings.ignoredOptionalDependencies
  const peerDependencyRules = pnpmSettings.peerDependencyRules
  const allowedDeprecatedVersions = pnpmSettings.allowedDeprecatedVersions
  const allowNonAppliedPatches = pnpmSettings.allowNonAppliedPatches
  let patchedDependencies = pnpmSettings.patchedDependencies
  if (patchedDependencies) {
    patchedDependencies = { ...patchedDependencies }
    for (const [dep, patchFile] of Object.entries(patchedDependencies)) {
      if (path.isAbsolute(patchFile)) continue
      patchedDependencies[dep] = path.join(manifestDir, patchFile)
    }
  }

  const supportedArchitectures = {
    os: pnpmSettings.supportedArchitectures?.os ?? ['current'],
    cpu: pnpmSettings.supportedArchitectures?.cpu ?? ['current'],
    libc: pnpmSettings.supportedArchitectures?.libc ?? ['current'],
  }

  const settings: OptionsFromRootManifest = {
    allowedDeprecatedVersions,
    allowNonAppliedPatches,
    configDependencies: pnpmSettings.configDependencies,
    neverBuiltDependencies,
    packageExtensions,
    ignoredOptionalDependencies,
    peerDependencyRules,
    patchedDependencies,
    supportedArchitectures,
    ignoredBuiltDependencies: pnpmSettings.ignoredBuiltDependencies,
  }
  if (onlyBuiltDependencies) {
    settings.onlyBuiltDependencies = onlyBuiltDependencies
  }
  if (onlyBuiltDependenciesFile) {
    settings.onlyBuiltDependenciesFile = path.join(manifestDir, onlyBuiltDependenciesFile)
  }
  return JSON.parse(JSON.stringify(settings))
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
