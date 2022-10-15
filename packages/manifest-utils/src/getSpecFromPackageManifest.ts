import { ProjectManifest } from '@pnpm/types'

export function getSpecFromPackageManifest (
  manifest: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies' | 'peerDependencies'>,
  depName: string
) {
  return manifest.optionalDependencies?.[depName] ??
    manifest.dependencies?.[depName] ??
    manifest.devDependencies?.[depName] ??
    manifest.peerDependencies?.[depName] ??
    ''
}
