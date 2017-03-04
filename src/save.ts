import {ignoreCache as readPkg} from './fs/readPkg'
import writePkg = require('write-pkg')
import {DependenciesType} from './getSaveType'
import {InstalledPackage} from './install/installMultiple'

export default async function save (pkgJsonPath: string, installedPackages: InstalledPackage[], saveType: DependenciesType, useExactVersion: boolean) {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = await readPkg(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.pkg.name] = semverCharacter + dependency.pkg.version
  })

  return writePkg(pkgJsonPath, packageJson)
}
