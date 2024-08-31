import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type SupportedArchitectures,
  type AllowedDeprecatedVersions,
  type PackageExtension,
  type PeerDependencyRules,
  type ProjectManifest,
} from '@pnpm/types'
import mapValues from 'ramda/src/map'

export interface OptionsFromRootManifest {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowNonAppliedPatches?: boolean
  overrides?: Record<string, string>
  overridesRefMap?: Record<string, string | undefined>
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  onlyBuiltDependenciesFile?: string
  packageExtensions?: Record<string, PackageExtension>
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: Record<string, string>
  peerDependencyRules?: PeerDependencyRules
  supportedArchitectures?: SupportedArchitectures
}

export function getOptionsFromRootManifest (manifestDir: string, manifest: ProjectManifest): OptionsFromRootManifest {
  // We read Yarn's resolutions field for compatibility
  // but we really replace the version specs to any other version spec, not only to exact versions,
  // so we cannot call it resolutions
  const replaceReferenceResult = mapValues(
    createVersionReferencesReplacer(manifest),
    {
      ...manifest.resolutions,
      ...manifest.pnpm?.overrides,
    }
  )
  const overrides = mapValues(x => x.spec, replaceReferenceResult)
  const overridesRefMapEntries = Object.entries(replaceReferenceResult)
    .map(([key, { refTarget }]) => [key, refTarget])
    .filter(([_key, refTarget]) => !!refTarget)
  const overridesRefMap = overridesRefMapEntries.length > 0 ? Object.fromEntries(overridesRefMapEntries) : undefined
  const neverBuiltDependencies = manifest.pnpm?.neverBuiltDependencies
  const onlyBuiltDependencies = manifest.pnpm?.onlyBuiltDependencies
  const onlyBuiltDependenciesFile = manifest.pnpm?.onlyBuiltDependenciesFile
  const packageExtensions = manifest.pnpm?.packageExtensions
  const ignoredOptionalDependencies = manifest.pnpm?.ignoredOptionalDependencies
  const peerDependencyRules = manifest.pnpm?.peerDependencyRules
  const allowedDeprecatedVersions = manifest.pnpm?.allowedDeprecatedVersions
  const allowNonAppliedPatches = manifest.pnpm?.allowNonAppliedPatches
  let patchedDependencies = manifest.pnpm?.patchedDependencies
  if (patchedDependencies) {
    patchedDependencies = { ...patchedDependencies }
    for (const [dep, patchFile] of Object.entries(patchedDependencies)) {
      if (path.isAbsolute(patchFile)) continue
      patchedDependencies[dep] = path.join(manifestDir, patchFile)
    }
  }

  const supportedArchitectures = {
    os: manifest.pnpm?.supportedArchitectures?.os ?? ['current'],
    cpu: manifest.pnpm?.supportedArchitectures?.cpu ?? ['current'],
    libc: manifest.pnpm?.supportedArchitectures?.libc ?? ['current'],
  }

  const settings: OptionsFromRootManifest = {
    allowedDeprecatedVersions,
    allowNonAppliedPatches,
    overrides,
    overridesRefMap,
    neverBuiltDependencies,
    packageExtensions,
    ignoredOptionalDependencies,
    peerDependencyRules,
    patchedDependencies,
    supportedArchitectures,
  }
  if (onlyBuiltDependencies) {
    settings.onlyBuiltDependencies = onlyBuiltDependencies
  }
  if (onlyBuiltDependenciesFile) {
    settings.onlyBuiltDependenciesFile = path.join(manifestDir, onlyBuiltDependenciesFile)
  }
  return settings
}

interface ReplaceReferenceResult {
  spec: string
  refTarget?: string
}

function createVersionReferencesReplacer (manifest: ProjectManifest): (spec: string) => ReplaceReferenceResult {
  const allDeps = {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
  return replaceVersionReferences.bind(null, allDeps)
}

function replaceVersionReferences (manifestDeps: Record<string, string>, spec: string): ReplaceReferenceResult {
  if (!spec.startsWith('$')) return { spec }
  const dependencyName = spec.slice(1)
  const newSpec = manifestDeps[dependencyName]
  if (newSpec) {
    return {
      spec: newSpec,
      refTarget: dependencyName,
    }
  }
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${spec} in overrides. The direct dependencies don't have dependency "${dependencyName}".`
  )
}
