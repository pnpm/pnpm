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
  const allOutdatedPkgs = mergeOutdatedPkgs(
    R.unnest(
      outdatedPkgsOfProjects.map(({ outdatedPackages }) => outdatedPackages),
    ),
  )

  if (R.isEmpty(allOutdatedPkgs)) {
    return []
  }
  const rowsGroupedByPkgs = Object.entries(allOutdatedPkgs)
    .sort(([pkgName1], [pkgName2]) => pkgName1.localeCompare(pkgName2))
    .map(([pkgName, outdatedPkgs]) => ({
      pkgName,
      rows: outdatedPkgsRows(Object.values(outdatedPkgs)),
    }))
  const renderedTable = alignColumns(
    R.unnest(rowsGroupedByPkgs.map(({ rows }) => rows)),
  )

  const choices = []
  let i = 0
  for (let { pkgName, rows } of rowsGroupedByPkgs) {
    choices.push({
      message: renderedTable
        .slice(i, i + rows.length)
        .join('\n    '),
      name: pkgName,
    })
    i += rows.length
  }
  return choices
}

function mergeOutdatedPkgs (outdatedPkgs: OutdatedPackage[]) {
  const allOutdatedPkgs: Record<string, Record<string, OutdatedPackage>> = {}
  for (const outdatedPkg of outdatedPkgs) {
    if (!allOutdatedPkgs[outdatedPkg.packageName]) {
      allOutdatedPkgs[outdatedPkg.packageName] = {}
    }
    const key = JSON.stringify([
      outdatedPkg.latestManifest?.version,
      outdatedPkg.current,
    ])
    if (!allOutdatedPkgs[outdatedPkg.packageName][key]) {
      allOutdatedPkgs[outdatedPkg.packageName][key] = outdatedPkg
      continue
    }
    if (allOutdatedPkgs[outdatedPkg.packageName][key].belongsTo === 'dependencies') continue
    if (outdatedPkg.belongsTo !== 'devDependencies') {
      allOutdatedPkgs[outdatedPkg.packageName][key].belongsTo = outdatedPkg.belongsTo
    }
  }
  return allOutdatedPkgs
}

function outdatedPkgsRows (outdatedPkgs: OutdatedPackage[]) {
  return outdatedPkgs
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
}

function alignColumns (rows: string[][]) {
  return table(
    rows,
    {
      border: getBorderCharacters('void'),
      columnDefault: {
        paddingLeft: 0,
        paddingRight: 1,
      },
      columns: {
        1: { alignment: 'right' },
      },
      drawHorizontalLine: () => false,
    },
  ).split('\n')
}
