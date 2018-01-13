import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import {DependenciesType, dependenciesTypes} from './getSaveType'
import {PackageJson} from '@pnpm/types'
import {packageJsonLogger} from './loggers'

export default async function (
  pkgJsonPath: string,
  removedPackages: string[],
  saveType?: DependenciesType
): Promise<PackageJson> {
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
  packageJsonLogger.debug({ updated: packageJson })
  return packageJson
}
