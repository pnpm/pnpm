import { PackageJson } from '@pnpm/types'

export default (manifest: PackageJson, depName: string) => {
  return manifest.dependencies && manifest.dependencies[depName]
    || manifest.devDependencies && manifest.devDependencies[depName]
    || manifest.optionalDependencies && manifest.optionalDependencies[depName]
    || ''
}
