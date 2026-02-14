import { stripVTControlCharacters } from 'util'
import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { type OutdatedPackage } from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import { getBorderCharacters, table } from '@zkochan/table'
import { pipe, groupBy, pluck, uniqBy, pickBy, and, isEmpty } from 'ramda'

export interface ChoiceRow {
  name: string
  value: string
  disabled?: boolean
}

type ChoiceGroup = Array<{
  name: string
  message: string
  choices: ChoiceRow[]
  disabled?: boolean
}>

export function getUpdateChoices (outdatedPkgsOfProjects: OutdatedPackage[], workspacesEnabled: boolean): ChoiceGroup {
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

  const finalChoices: ChoiceGroup = []
  for (const [depGroup, choiceRows] of Object.entries(groupPkgsByType)) {
    if (choiceRows.length === 0) continue
    const rawChoices: RawChoice[] = []
    for (const choice of choiceRows) {
      // The list of outdated dependencies also contains deprecated packages.
      // But we only want to show those dependencies that have newer versions.
      if (choice.latestManifest?.version !== choice.current) {
        rawChoices.push(buildPkgChoice(choice, workspacesEnabled))
      }
    }
    if (rawChoices.length === 0) continue
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
        name: outdatedPkg.name,
        message: renderedTable[i],
        value: outdatedPkg.name,
      }
    })

    // To filter out selected "dependencies" or "devDependencies" in the final output,
    // we rename it here to "[dependencies]" or "[devDependencies]",
    // which will be filtered out in the format function of the prompt.
    finalChoices.push({ name: `[${depGroup}]`, choices, message: depGroup })
  }
  return finalChoices
}

interface RawChoice {
  raw: string[]
  name: string
  disabled?: boolean
}

function buildPkgChoice (outdatedPkg: OutdatedPackage, workspacesEnabled: boolean): RawChoice {
  const sdiff = semverDiff.default(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
  const nextVersion = sdiff.change === null
    ? outdatedPkg.latestManifest!.version
    : colorizeSemverDiff.default(sdiff as any) // eslint-disable-line @typescript-eslint/no-explicit-any
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

function getPkgUrl (pkg: OutdatedPackage): string {
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

function alignColumns (rows: string[][]): string[] {
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
            1: { width: getColumnWidth(rows, 1, 15), alignment: 'right' },
            3: { width: getColumnWidth(rows, 3, 15) },
            4: { paddingLeft: 2 },
            5: { paddingLeft: 2 },
          },
      drawHorizontalLine: () => false,
    }
  ).split('\n')
}

function getColumnWidth (rows: string[][], columnIndex: number, minWidth: number): number {
  return rows.reduce((max, row) => {
    if (row[columnIndex] == null) return max
    return Math.max(max, stripVTControlCharacters(row[columnIndex]).length)
  }, minWidth)
}
