import { ProjectManifest, DependenciesManifestField } from '@pnpm/types'

export function getSpecFromPackageManifest (
  manifest: Pick<ProjectManifest, DependenciesManifestField>,
  depName: string
) {
  return manifest.optionalDependencies?.[depName] ??
    manifest.dependencies?.[depName] ??
    manifest.devDependencies?.[depName] ??
    manifest.peerDependencies?.[depName] ??
    ''
}
