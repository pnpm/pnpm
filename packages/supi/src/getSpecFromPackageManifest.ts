import { ImporterManifest } from '@pnpm/types'

export default (
  manifest: Pick<ImporterManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
  depName: string,
) => {
  return manifest.dependencies && manifest.dependencies[depName]
    || manifest.devDependencies && manifest.devDependencies[depName]
    || manifest.optionalDependencies && manifest.optionalDependencies[depName]
    || ''
}
