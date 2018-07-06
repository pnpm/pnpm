import {PackageJson} from '@pnpm/types'
import {
  DependenciesType,
  dependenciesTypes,
  packageJsonLogger,
} from '@pnpm/utils'
import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')

export default async function (
  pkgJsonPath: string,
  removedPackages: string[],
  opts: {
    saveType?: DependenciesType,
    prefix: string,
  },
): Promise<PackageJson> {
  const packageJson = await loadJsonFile(pkgJsonPath)

  if (opts.saveType) {
    packageJson[opts.saveType] = packageJson[opts.saveType]

    if (!packageJson[opts.saveType]) return packageJson

    removedPackages.forEach((dependency) => {
      delete packageJson[opts.saveType as DependenciesType][dependency]
    })
  } else {
    dependenciesTypes
      .filter((deptype) => packageJson[deptype])
      .forEach((deptype) => {
        removedPackages.forEach((dependency) => {
          delete packageJson[deptype][dependency]
        })
      })
  }

  await writePkg(pkgJsonPath, packageJson)
  packageJsonLogger.debug({
    prefix: opts.prefix,
    updated: packageJson,
  })
  return packageJson
}
