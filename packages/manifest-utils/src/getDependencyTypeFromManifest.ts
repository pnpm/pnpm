import { ProjectManifest, DependenciesOrPeersField } from '@pnpm/types'

export function getDependencyTypeFromManifest (
  manifest: Pick<ProjectManifest, DependenciesOrPeersField>,
  depName: string
): DependenciesOrPeersField | null {
  if (manifest.optionalDependencies?.[depName]) return 'optionalDependencies'
  if (manifest.dependencies?.[depName]) return 'dependencies'
  if (manifest.devDependencies?.[depName]) return 'devDependencies'
  if (manifest.peerDependencies?.[depName]) return 'peerDependencies'
  return null
}
