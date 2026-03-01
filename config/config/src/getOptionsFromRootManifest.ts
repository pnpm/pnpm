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
import { map as mapValues, pick } from 'ramda'

export type OptionsFromRootManifest = {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowUnusedPatches?: boolean
  overrides?: Record<string, string>
  packageExtensions?: Record<string, PackageExtension>
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: Record<string, string>
  peerDependencyRules?: PeerDependencyRules
  supportedArchitectures?: SupportedArchitectures
  allowBuilds?: Record<string, boolean | string>
  requiredScripts?: string[]
} & Pick<PnpmSettings, 'configDependencies' | 'auditConfig' | 'updateConfig'>

export function getOptionsFromRootManifest (manifestDir: string, manifest: ProjectManifest): OptionsFromRootManifest {
  const settings: OptionsFromRootManifest = getOptionsFromPnpmSettings(manifestDir, {
    ...pick([
      'allowBuilds',
      'allowUnusedPatches',
      'allowedDeprecatedVersions',
      'auditConfig',
      'configDependencies',
      'ignoredOptionalDependencies',
      'overrides',
      'packageExtensions',
      'patchedDependencies',
      'peerDependencyRules',
      'requiredScripts',
      'supportedArchitectures',
      'updateConfig',
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

export function getOptionsFromPnpmSettings (manifestDir: string | undefined, pnpmSettings: PnpmSettings, manifest?: ProjectManifest): OptionsFromRootManifest {
  const settings: OptionsFromRootManifest = replaceEnvInSettings(pnpmSettings)
  if (settings.overrides) {
    if (Object.keys(settings.overrides).length === 0) {
      delete settings.overrides
    } else if (manifest) {
      settings.overrides = mapValues(createVersionReferencesReplacer(manifest), settings.overrides)
    }
  }
  if (pnpmSettings.patchedDependencies) {
    settings.patchedDependencies = { ...pnpmSettings.patchedDependencies }
    for (const [dep, patchFile] of Object.entries(pnpmSettings.patchedDependencies)) {
      if (manifestDir == null || path.isAbsolute(patchFile)) continue
      settings.patchedDependencies[dep] = path.join(manifestDir, patchFile)
    }
  }

  return settings
}

function replaceEnvInSettings (settings: PnpmSettings): PnpmSettings {
  const newSettings: PnpmSettings = {}
  for (const [key, value] of Object.entries(settings)) {
    let newKey: string
    try {
      newKey = envReplace(key, process.env)
    } catch (err) {
      globalWarn((err as Error).message)
      newKey = key
    }
    if (typeof value === 'string') {
      let newValue: string
      try {
        newValue = envReplace(value, process.env)
      } catch (err) {
        globalWarn((err as Error).message)
        newValue = value
      }
      // @ts-expect-error
      newSettings[newKey as keyof PnpmSettings] = newValue
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
