import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { type OutdatedPackage } from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import { getBorderCharacters, table } from '@zkochan/table'
import { pipe, groupBy, pluck, uniqBy, pickBy, and } from 'ramda'
import isEmpty from 'ramda/src/isEmpty'

export interface ChoiceRow {
  name: string
  value: string
  disabled?: boolean
}

type ChoiceGroup = Array<{
  name: string
  choices: ChoiceRow[]
}>

export function getUpdateChoices (outdatedPkgsOfProjects: OutdatedPackage[], workspacesEnabled: boolean) {
  if (isEmpty(outdatedPkgsOfProjects)) {
    return []
  }

  const pkgUniqueKey = (outdatedPkg: OutdatedPackage) => {
    return JSON.stringify([outdatedPkg.packageName, outdatedPkg.latestManifest?.version, outdatedPkg.current])
  }

  const dedupeAndGroupPkgs = pipe(
    uniqBy((outdatedPkg: OutdatedPackage) => pkgUniqueKey(outdatedPkg)),
    groupBy((outdatedPkg: OutdatedPackage) => outdatedPkg.belongsTo)
  )

  const groupPkgsByType = dedupeAndGroupPkgs(outdatedPkgsOfProjects)

  const headerRow = {
    Package: true,
    Current: true,
    ' ': true,
    Target: true,
    Workspace: workspacesEnabled,
    URL: true,
  }
  // returns only the keys that are true
  const header: string[] = Object.keys(pickBy(and, headerRow))

  return Object.entries(groupPkgsByType).reduce((finalChoices: ChoiceGroup, [depGroup, choiceRows]) => {
    if (choiceRows.length === 0) {
      return finalChoices
    }

    const rawChoices = choiceRows.map(choice => buildPkgChoice(choice, workspacesEnabled))
    // add in a header row for each group
    rawChoices.unshift({
      raw: header,
      name: '',
      disabled: true,
    })
    const renderedTable = alignColumns(pluck('raw', rawChoices)).filter(Boolean)

    const choices = rawChoices.map((outdatedPkg, i) => {
      if (i === 0) {
        return {
          name: renderedTable[i],
          value: '',
          disabled: true,
          hint: '',
        }
      }
      return {
        name: renderedTable[i],
        value: outdatedPkg.name,
      }
    })

    finalChoices.push({ name: depGroup, choices })

    return finalChoices
  }, [])
}

function buildPkgChoice (outdatedPkg: OutdatedPackage, workspacesEnabled: boolean): { raw: string[], name: string, disabled?: boolean } {
  const sdiff = semverDiff(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
  const nextVersion = sdiff.change === null
    ? outdatedPkg.latestManifest!.version
    : colorizeSemverDiff(sdiff as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  const label = outdatedPkg.packageName

  const lineParts = {
    label,
    current: outdatedPkg.current,
    arrow: '❯',
    nextVersion,
    workspace: outdatedPkg.workspace,
    url: getPkgUrl(outdatedPkg),
  }

  if (!workspacesEnabled) {
    delete lineParts.workspace
  }

  return {
    raw: Object.values(lineParts),
    name: outdatedPkg.packageName,
  }
}

function getPkgUrl (pkg: OutdatedPackage) {
  if (pkg.latestManifest?.homepage) {
    return pkg.latestManifest?.homepage
  }

  if (typeof pkg.latestManifest?.repository !== 'string') {
    if (pkg.latestManifest?.repository?.url) {
      return pkg.latestManifest?.repository?.url
    }
  }

  return ''
}

function alignColumns (rows: string[][]) {
  return table(
    rows,
    {
      border: getBorderCharacters('void'),
      columnDefault: {
        paddingLeft: 0,
        paddingRight: 1,
        wrapWord: true,
      },
      columns:
          {
            0: { width: 50, truncate: 100 },
            1: { width: 15, alignment: 'right' },
            3: { width: 15 },
            4: { paddingLeft: 2 },
            5: { paddingLeft: 2 },
          },
      drawHorizontalLine: () => false,
    }
  ).split('\n')
}
