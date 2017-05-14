import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import {DependenciesType} from './getSaveType'
import {Package} from './types'

export default async function (
  pkgJsonPath: string,
  removedPackages: string[],
  saveType: DependenciesType
): Promise<Package> {
  const packageJson = await loadJsonFile(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType]

  if (!packageJson[saveType]) return packageJson

  removedPackages.forEach(dependency => {
    delete packageJson[saveType][dependency]
  })

  await writePkg(pkgJsonPath, packageJson)
  return packageJson
}
