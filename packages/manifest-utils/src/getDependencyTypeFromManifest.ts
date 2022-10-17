import { ProjectManifest, DependenciesManifestField } from '@pnpm/types'

export const getDependencyTypeFromManifest = (
  manifest: Pick<ProjectManifest, DependenciesManifestField>,
  depName: string
): DependenciesManifestField | null => {
  if (manifest.optionalDependencies?.[depName]) return 'optionalDependencies'
  else if (manifest.peerDependencies?.[depName]) return 'peerDependencies'
  else if (manifest.dependencies?.[depName]) return 'dependencies'
  else if (manifest.devDependencies?.[depName]) return 'devDependencies'
  else return null
}
