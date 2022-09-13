import {
  AllowedDeprecatedVersions,
  PackageExtension,
  PeerDependencyRules,
  ProjectManifest,
} from '@pnpm/types'

export default function getOptionsFromRootManifest (manifest: ProjectManifest): {
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
  const overrides = manifest.pnpm?.overrides ?? manifest.resolutions
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
