import requireJson from './fs/requireJson'
import writeJson from './fs/writeJson'
import sortedObject = require('sorted-object')
import {DependenciesType} from './getSaveType'
import {PackageContext} from './install'

export default function save (pkgJsonPath: string, installedPackages: PackageContext[], saveType: DependenciesType, useExactVersion: boolean) {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = requireJson(pkgJsonPath, { ignoreCache: true })
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.spec.name] = semverCharacter + dependency.version
  })
  packageJson[saveType] = sortedObject(packageJson[saveType])

  return writeJson(pkgJsonPath, packageJson)
}
