import { ProjectManifest } from '@pnpm/types'

export default function getOptionsFromRootManifest (manifest: ProjectManifest) {
  // We read Yarn's resolutions field for compatibility
  // but we really replace the version specs to any other version spec, not only to exact versions,
  // so we cannot call it resolutions
  const overrides = manifest.pnpm?.overrides ?? manifest.resolutions
  const neverBuiltDependencies = manifest.pnpm?.neverBuiltDependencies ?? []
  const packageExtensions = manifest.pnpm?.packageExtensions
  return {
    overrides,
    neverBuiltDependencies,
    packageExtensions,
  }
}
