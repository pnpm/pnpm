import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { OutdatedPackage } from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import { ProjectManifest } from '@pnpm/types'
import R = require('ramda')
import { getBorderCharacters, table } from 'table'

export default function (outdatedPkgsOfProjects: Array<{
  manifest: ProjectManifest,
  outdatedPackages: OutdatedPackage[],
  prefix: string,
}>) {
  const allOutdatedPkgs: Record<string, Record<string, OutdatedPackage>> = {}
  R.unnest(
    outdatedPkgsOfProjects.map(({ outdatedPackages }) => outdatedPackages),
  )
    .forEach((outdatedPkg) => {
      if (!allOutdatedPkgs[outdatedPkg.packageName]) {
        allOutdatedPkgs[outdatedPkg.packageName] = {}
      }
      const key = JSON.stringify([
        outdatedPkg.latestManifest?.version,
        outdatedPkg.current,
      ])
      if (!allOutdatedPkgs[outdatedPkg.packageName][key]) {
        allOutdatedPkgs[outdatedPkg.packageName][key] = outdatedPkg
        return
      }
      if (allOutdatedPkgs[outdatedPkg.packageName][key].belongsTo === 'dependencies') return
      if (outdatedPkg.belongsTo !== 'devDependencies') {
        allOutdatedPkgs[outdatedPkg.packageName][key].belongsTo = outdatedPkg.belongsTo
      }
    })

  if (R.isEmpty(allOutdatedPkgs)) {
    return []
  }
  const rows = Object.entries(allOutdatedPkgs)
    .sort(([pkgName1], [pkgName2]) => pkgName1.localeCompare(pkgName2))
    .map(([packageName, outdatedPkgs]) => {
      const columns = Object.values(outdatedPkgs)
        .map((outdatedPkg) => {
          const sdiff = semverDiff(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
          const nextVersion = sdiff.change === null
            ? outdatedPkg.latestManifest!.version
            : colorizeSemverDiff(sdiff as any) // tslint:disable-line:no-any
          let label = outdatedPkg.packageName
          switch (outdatedPkg.belongsTo) {
            case 'devDependencies': {
              label += ' (dev)'
              break
            }
            case 'optionalDependencies': {
              label += ' (optional)'
              break
            }
          }
          return [label, outdatedPkg.current, '❯', nextVersion]
        })
      return {
        columns,
        name: packageName,
      }
    })
  const renderedTable = table(
    R.unnest(rows.map(({ columns }) => columns)),
    {
      border: getBorderCharacters('void'),
      columnDefault: {
        paddingLeft: 0,
        paddingRight: 1,
      },
      columns: {
        1: { alignment: 'right' },
      },
      drawHorizontalLine: () => {
          return false
      },
    },
  ).split('\n')

  const choices = []
  let i = 0
  for (let row of rows) {
    choices.push({
      message: renderedTable
        .slice(i, i + row.columns.length)
        .join('\n    '),
      name: row.name,
    })
    i += row.columns.length
  }
  return choices
}
