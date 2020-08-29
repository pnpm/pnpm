import { ProjectManifest } from '@pnpm/types'

export default (
  manifest: Pick<ProjectManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
  depName: string
) => {
  return manifest.optionalDependencies?.[depName] ??
    manifest.dependencies?.[depName] ??
    manifest.devDependencies?.[depName] ??
    ''
}
