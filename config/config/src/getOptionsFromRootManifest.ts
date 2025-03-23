import path from 'path'
import { envReplace } from '@pnpm/config.env-replace'
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
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import { globalWarn } from '@pnpm/logger'

export type OptionsFromRootManifest = {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowUnusedPatches?: boolean
  ignorePatchFailures?: boolean
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
    ...pick([
      'allowedDeprecatedVersions',
      'allowNonAppliedPatches',
      'allowUnusedPatches',
      'configDependencies',
      'ignoredBuiltDependencies',
      'ignoredOptionalDependencies',
      'ignorePatchFailures',
      'neverBuiltDependencies',
      'onlyBuiltDependencies',
      'onlyBuiltDependenciesFile',
      'overrides',
      'packageExtensions',
      'patchedDependencies',
      'peerDependencyRules',
      'supportedArchitectures',
    ], manifest.pnpm ?? {}),
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
  const renamedKeys = ['allowNonAppliedPatches'] as const satisfies Array<keyof PnpmSettings>
  const settings: OptionsFromRootManifest = omit(renamedKeys, replaceEnvInSettings(pnpmSettings))
  if (settings.overrides) {
    if (Object.keys(settings.overrides).length === 0) {
      delete settings.overrides
    } else if (manifest) {
      settings.overrides = mapValues(createVersionReferencesReplacer(manifest), settings.overrides)
    }
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
  if (pnpmSettings.allowNonAppliedPatches != null) {
    globalWarn('allowNonAppliedPatches is deprecated, use allowUnusedPatches instead.')
    settings.allowUnusedPatches ??= pnpmSettings.allowNonAppliedPatches
  }
  if (pnpmSettings.ignorePatchFailures != null) {
    settings.ignorePatchFailures = pnpmSettings.ignorePatchFailures
  }
  return settings
}

function replaceEnvInSettings (settings: PnpmSettings): PnpmSettings {
  const newSettings: PnpmSettings = {}
  for (const [key, value] of Object.entries(settings)) {
    const newKey = envReplace(key, process.env)
    if (typeof value === 'string') {
      // @ts-expect-error
      newSettings[newKey as keyof PnpmSettings] = envReplace(value, process.env)
    } else {
      newSettings[newKey as keyof PnpmSettings] = value
    }
  }
  return newSettings
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
