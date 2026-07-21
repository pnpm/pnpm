import { stripVTControlCharacters } from 'node:util'

import { colorizeSemverDiff } from '@pnpm/colorize-semver-diff'
import type { OutdatedPackage } from '@pnpm/deps.inspection.outdated'
import { semverDiff } from '@pnpm/semver-diff'
import { getBorderCharacters, table } from '@zkochan/table'
import { and, groupBy, isEmpty, pickBy, pipe, pluck, uniqBy } from 'ramda'

export interface ChoiceRow {
  name: string
  value: string
  message: string
  disabled?: boolean
}

type ChoiceGroup = Array<{
  name: string
  message: string
  choices: ChoiceRow[]
  disabled?: boolean
}>

type UpdateChoiceDependency = OutdatedPackage & { dependencyType?: 'githubAction' }

export function getUpdateChoices (outdatedPkgsOfProjects: UpdateChoiceDependency[], workspacesEnabled: boolean): ChoiceGroup {
  if (isEmpty(outdatedPkgsOfProjects)) {
    return []
  }

  const pkgUniqueKey = (outdatedPkg: UpdateChoiceDependency) => {
    return JSON.stringify([outdatedPkg.packageName, outdatedPkg.latestManifest?.version, outdatedPkg.current, outdatedPkg.dependencyType])
  }

  const dedupeAndGroupPkgs = pipe(
    uniqBy((outdatedPkg: UpdateChoiceDependency) => pkgUniqueKey(outdatedPkg)),
    groupBy((outdatedPkg: UpdateChoiceDependency) => outdatedPkg.dependencyType ?? outdatedPkg.belongsTo)
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
      // The list of outdated dependencies also contains deprecated packages
      // and entries from registries we cannot resolve against (no manifest).
      // We only want to show those dependencies that have a known newer version.
      if (choice.latestManifest != null && choice.latestManifest.version !== choice.current) {
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
          message: renderedTable[i],
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

    // The prompt renderer treats bracketed names as group labels rather than selectable values.
    finalChoices.push({
      name: `[${depGroup}]`,
      choices,
      message: depGroup === 'githubAction' ? 'GitHub Actions' : depGroup,
    })
  }
  return finalChoices
}

interface RawChoice {
  raw: string[]
  name: string
  disabled?: boolean
}

function buildPkgChoice (outdatedPkg: UpdateChoiceDependency, workspacesEnabled: boolean): RawChoice {
  const sdiff = semverDiff(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
  const nextVersion = sdiff.change === null
    ? outdatedPkg.latestManifest!.version
    : colorizeSemverDiff(sdiff as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  const label = outdatedPkg.packageName

  const raw: string[] = [
    label,
    outdatedPkg.current ?? '',
    '❯',
    nextVersion,
  ]
  if (workspacesEnabled) {
    raw.push(outdatedPkg.workspace ?? '')
  }
  raw.push(getPkgUrl(outdatedPkg))

  return {
    raw,
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
