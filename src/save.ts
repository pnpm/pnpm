import readPkg = require('read-pkg')
import writePkg = require('write-pkg')
import {DependenciesType} from './getSaveType'
import {Package} from './types'
import {PackageSpec} from './resolve'

export default async function save (
  pkgJsonPath: string,
  packageSpecs: ({
    name: string,
    saveSpec: string,
  })[],
  saveType: DependenciesType
): Promise<Package> {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = await readPkg(pkgJsonPath, {normalize: false})
  packageJson[saveType] = packageJson[saveType] || {}
  packageSpecs.forEach(dependency => {
    packageJson[saveType][dependency.name] = dependency.saveSpec
  })

  await writePkg(pkgJsonPath, packageJson)
  return packageJson
}
