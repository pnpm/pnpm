import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { OutdatedPackage } from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import { DependenciesField, ProjectManifest } from '@pnpm/types'
import R = require('ramda')

const DEPS_PRIORITY: Record<DependenciesField, number> = {
  'dependencies': 0,
  'devDependencies': 2,
  'optionalDependencies': 1,
}

export default function (outdatedPkgsOfProjects: Array<{
  manifest: ProjectManifest,
  outdatedPackages: OutdatedPackage[],
  prefix: string,
}>) {
  const allOutdatedPkgs: Record<string, OutdatedPackage> = {}
  R.flatten(
    outdatedPkgsOfProjects.map(({ outdatedPackages }) => outdatedPackages),
  )
    .forEach((outdatedPkg) => {
      const key = JSON.stringify([
        outdatedPkg.packageName,
        outdatedPkg.latestManifest?.version,
        outdatedPkg.current,
      ])
      if (!allOutdatedPkgs[key]) {
        allOutdatedPkgs[key] = outdatedPkg
        return
      }
      if (allOutdatedPkgs[key].belongsTo === 'dependencies') return
      if (outdatedPkg.belongsTo !== 'devDependencies') {
        allOutdatedPkgs[key].belongsTo = outdatedPkg.belongsTo
      }
    })
  const outdatedPackages = Object.values(allOutdatedPkgs)

  if (outdatedPackages.length === 0) {
    return []
  }
  const outdatedPackagesByType = R.groupBy(R.prop('belongsTo'), outdatedPackages)
  return Object.entries(outdatedPackagesByType)
    .sort(([depType1], [depType2]) => DEPS_PRIORITY[depType1] - DEPS_PRIORITY[depType2])
    .map(([depType, outdatedPkgs]) => ({
      choices: Object.entries(R.groupBy(R.prop('packageName'), outdatedPkgs))
        .map(([packageName, outdatedPkgs]) => {
          const message = outdatedPkgs
            .map((outdatedPkg) => {
              const sdiff = semverDiff(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
              const nextVersion = sdiff.change === null
                ? outdatedPkg.latestManifest!.version
                : colorizeSemverDiff(sdiff as any) // tslint:disable-line:no-any
              return `${outdatedPkg.packageName} ${outdatedPkg.current} ❯ ${nextVersion}`
            }).join('\n    ')
          return {
            message,
            name: packageName,
          }
        }),
      name: depType,
    }))
}
