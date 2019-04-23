import { packageJsonLogger } from '@pnpm/core-loggers'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  PackageJson,
} from '@pnpm/types'

export default async function (
  packageJson: PackageJson,
  removedPackages: string[],
  opts: {
    saveType?: DependenciesField,
    prefix: string,
  },
): Promise<PackageJson> {
  if (opts.saveType) {
    packageJson[opts.saveType] = packageJson[opts.saveType]

    if (!packageJson[opts.saveType]) return packageJson

    removedPackages.forEach((dependency) => {
      delete packageJson[opts.saveType as DependenciesField]![dependency]
    })
  } else {
    DEPENDENCIES_FIELDS
      .filter((depField) => packageJson[depField])
      .forEach((depField) => {
        removedPackages.forEach((dependency) => {
          delete packageJson[depField]![dependency]
        })
      })
  }
  if (packageJson.peerDependencies) {
    for (const removedDependency of removedPackages) {
      delete packageJson.peerDependencies[removedDependency]
    }
  }

  packageJsonLogger.debug({
    prefix: opts.prefix,
    updated: packageJson,
  })
  return packageJson
}
