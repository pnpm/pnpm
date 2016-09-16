import requireJson from './fs/requireJson'
import writeJson from './fs/writeJson'
import sortedObject = require('sorted-object')
import {DependenciesType} from './getSaveType'
import {InstalledPackage} from './install'

export default function save (pkgJsonPath: string, installedPackages: InstalledPackage[], saveType: DependenciesType, useExactVersion: boolean) {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = requireJson(pkgJsonPath, { ignoreCache: true })
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.pkg.name] = semverCharacter + dependency.pkg.version
  })
  packageJson[saveType] = sortedObject(packageJson[saveType])

  return writeJson(pkgJsonPath, packageJson)
}
