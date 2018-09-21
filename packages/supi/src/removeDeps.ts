import { packageJsonLogger } from '@pnpm/core-loggers'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  PackageJson,
} from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')

export default async function (
  pkgJsonPath: string,
  removedPackages: string[],
  opts: {
    saveType?: DependenciesField,
    prefix: string,
  },
): Promise<PackageJson> {
  const packageJson = await loadJsonFile(pkgJsonPath)

  if (opts.saveType) {
    packageJson[opts.saveType] = packageJson[opts.saveType]

    if (!packageJson[opts.saveType]) return packageJson

    removedPackages.forEach((dependency) => {
      delete packageJson[opts.saveType as DependenciesField][dependency]
    })
  } else {
    DEPENDENCIES_FIELDS
      .filter((depField) => packageJson[depField])
      .forEach((depField) => {
        removedPackages.forEach((dependency) => {
          delete packageJson[depField][dependency]
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
