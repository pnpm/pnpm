import { ImporterManifest } from '@pnpm/types'

export default (
  manifest: Pick<ImporterManifest, 'devDependencies' | 'dependencies' | 'optionalDependencies'>,
  depName: string,
) => {
  return manifest.dependencies?.[depName]
    || manifest.devDependencies?.[depName]
    || manifest.optionalDependencies?.[depName]
    || ''
}
