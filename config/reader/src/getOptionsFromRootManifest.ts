import path from 'node:path'

import { envReplace } from '@pnpm/config.env-replace'
import { PnpmError } from '@pnpm/error'
import type {
  AllowedDeprecatedVersions,
  PackageExtension,
  PeerDependencyRules,
  PnpmSettings,
  ProjectManifest,
  SupportedArchitectures,
} from '@pnpm/types'
import { map as mapValues } from 'ramda'

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
} & Pick<PnpmSettings, 'configDependencies' | 'auditConfig' | 'agent' | 'updateConfig'>

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
