import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import {DependenciesType, dependenciesTypes} from './getSaveType'
import {Package} from './types'

export default async function (
  pkgJsonPath: string,
  removedPackages: string[],
  saveType?: DependenciesType
): Promise<Package> {
  const packageJson = await loadJsonFile(pkgJsonPath)

  if (saveType) {
    packageJson[saveType] = packageJson[saveType]

    if (!packageJson[saveType]) return packageJson

    removedPackages.forEach(dependency => {
      delete packageJson[saveType][dependency]
    })
  } else {
    dependenciesTypes
      .filter(deptype => packageJson[deptype])
      .forEach(deptype => {
        removedPackages.forEach(dependency => {
          delete packageJson[deptype][dependency]
        })
      })
  }

  await writePkg(pkgJsonPath, packageJson)
  return packageJson
}
