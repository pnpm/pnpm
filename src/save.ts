import {ignoreCache as readPkg} from './fs/readPkg'
import writePkg = require('write-pkg')
import {DependenciesType} from './getSaveType'
import {Package} from './types'

export default async function save (
  pkgJsonPath: string,
  installedPackages: Package[],
  saveType: DependenciesType,
  useExactVersion: boolean
): Promise<Package> {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = await readPkg(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.name] = semverCharacter + dependency.version
  })

  await writePkg(pkgJsonPath, packageJson)
  return packageJson
}
