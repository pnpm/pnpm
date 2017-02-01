import {ignoreCache as requireJson} from './fs/requireJson'
import writePkg = require('write-pkg')
import sortedObject = require('sorted-object')
import {DependenciesType} from './getSaveType'
import {InstalledPackage} from './types'

export default async function save (pkgJsonPath: string, installedPackages: InstalledPackage[], saveType: DependenciesType, useExactVersion: boolean) {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = await requireJson(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.pkg.name] = semverCharacter + dependency.pkg.version
  })
  packageJson[saveType] = sortedObject(packageJson[saveType])

  return writePkg(pkgJsonPath, packageJson)
}
