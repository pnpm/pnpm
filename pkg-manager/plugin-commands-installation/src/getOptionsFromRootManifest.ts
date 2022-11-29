import { PnpmError } from '@pnpm/error'
import {
  AllowedDeprecatedVersions,
  PackageExtension,
  PeerDependencyRules,
  ProjectManifest,
} from '@pnpm/types'
import mapValues from 'ramda/src/map'

export function getOptionsFromRootManifest (manifest: ProjectManifest): {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowNonAppliedPatches?: boolean
  overrides?: Record<string, string>
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  packageExtensions?: Record<string, PackageExtension>
  patchedDependencies?: Record<string, string>
  peerDependencyRules?: PeerDependencyRules
} {
  // We read Yarn's resolutions field for compatibility
  // but we really replace the version specs to any other version spec, not only to exact versions,
  // so we cannot call it resolutions
  const overrides = mapValues(
    createVersionReferencesReplacer(manifest),
    manifest.pnpm?.overrides ?? manifest.resolutions ?? {}
  )
  const neverBuiltDependencies = manifest.pnpm?.neverBuiltDependencies
  const onlyBuiltDependencies = manifest.pnpm?.onlyBuiltDependencies
  const packageExtensions = manifest.pnpm?.packageExtensions
  const peerDependencyRules = manifest.pnpm?.peerDependencyRules
  const allowedDeprecatedVersions = manifest.pnpm?.allowedDeprecatedVersions
  const allowNonAppliedPatches = manifest.pnpm?.allowNonAppliedPatches
  const patchedDependencies = manifest.pnpm?.patchedDependencies
  const settings = {
    allowedDeprecatedVersions,
    allowNonAppliedPatches,
    overrides,
    neverBuiltDependencies,
    packageExtensions,
    peerDependencyRules,
    patchedDependencies,
  }
  if (onlyBuiltDependencies) {
    settings['onlyBuiltDependencies'] = onlyBuiltDependencies
  }
  return settings
}

function createVersionReferencesReplacer (manifest: ProjectManifest) {
  const allDeps = {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
  return replaceVersionReferences.bind(null, allDeps)
}

function replaceVersionReferences (dep: Record<string, string>, spec: string) {
  if (!spec.startsWith('$')) return spec
  const dependencyName = spec.slice(1)
  const newSpec = dep[dependencyName]
  if (newSpec) return newSpec
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${spec} in overrides. The direct dependencies don't have dependency "${dependencyName}".`
  )
}
