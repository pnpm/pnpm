import { colorizeSemverDiff } from '@pnpm/colorize-semver-diff'
import type { OutdatedPackage } from '@pnpm/deps.inspection.outdated'
import { semverDiff } from '@pnpm/semver-diff'
import { getBorderCharacters, table } from '@zkochan/table'
import { and, groupBy, isEmpty, pickBy, pluck } from 'ramda'
import stringWidth from 'string-width'

export interface ChoiceRow {
  name: string
  value: string
  message: string
  short: string
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

  // Entries that differ only by the project they came from collapse into
  // one choice, because selecting it updates the package in every
  // project. Their workspaces are collected onto the survivor so the
  // Workspace column names all of them rather than whichever came first.
  const deduped: UpdateChoiceDependency[] = []
  const workspacesByKey = new Map<string, Set<string>>()
  for (const outdatedPkg of outdatedPkgsOfProjects) {
    const key = pkgUniqueKey(outdatedPkg)
    let workspaces = workspacesByKey.get(key)
    if (workspaces == null) {
      workspaces = new Set()
      workspacesByKey.set(key, workspaces)
      deduped.push(outdatedPkg)
    }
    if (outdatedPkg.workspace) {
      workspaces.add(outdatedPkg.workspace)
    }
  }

  const groupPkgsByType = groupBy(
    (outdatedPkg: UpdateChoiceDependency) => outdatedPkg.dependencyType ?? outdatedPkg.belongsTo,
    deduped
  )

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
        rawChoices.push(buildPkgChoice(choice, workspacesEnabled, workspacesByKey.get(pkgUniqueKey(choice))))
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
          short: '',
          disabled: true,
          hint: '',
        }
      }
      return {
        name: outdatedPkg.name,
        message: renderedTable[i],
        value: outdatedPkg.name,
        short: sanitizeUpdateChoiceText(outdatedPkg.name),
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

function buildPkgChoice (outdatedPkg: UpdateChoiceDependency, workspacesEnabled: boolean, workspaces?: Set<string>): RawChoice {
  const sdiff = semverDiff(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
  const nextVersion = sdiff.change === null
    ? outdatedPkg.latestManifest!.version
    : colorizeSemverDiff(sdiff as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  const label = outdatedPkg.packageName

  const raw: string[] = [
    sanitizeUpdateChoiceText(label),
    outdatedPkg.current ?? '',
    '❯',
    // Not sanitized: `colorizeSemverDiff` puts the highlighting escapes
    // in here deliberately.
    nextVersion,
  ]
  if (workspacesEnabled) {
    raw.push(Array.from(workspaces ?? []).map(sanitizeUpdateChoiceText).join(', '))
  }
  raw.push(sanitizeUpdateChoiceText(getPkgUrl(outdatedPkg)))

  return {
    raw,
    name: outdatedPkg.packageName,
  }
}

/**
 * Strip control and formatting characters from text that goes into a
 * single-line table cell.
 */
export function sanitizeUpdateChoiceText (text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/[\u0000-\u001F\u007F-\u009F\u00AD\u0600-\u0605\u061C\u06DD\u070F\u0890\u0891\u08E2\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u206F\uFEFF\uFFF9-\uFFFB\u{110BD}\u{110CD}\u{13430}-\u{1343F}\u{1BCA0}-\u{1BCA3}\u{1D173}-\u{1D17A}\u{E0001}\u{E0020}-\u{E007F}]/gu, '')
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

/**
 * The width the column has to be given so that none of its cells wrap.
 *
 * Measured in terminal columns rather than in code units, matching how
 * `@zkochan/table` lays the cell out: a cell that renders wider than the
 * width it is given wraps, which splits the row and shifts every choice
 * after it. `stringWidth` also discards the highlighting escapes
 * `colorizeSemverDiff` puts in the target column.
 */
function getColumnWidth (rows: string[][], columnIndex: number, minWidth: number): number {
  return rows.reduce((max, row) => {
    if (row[columnIndex] == null) return max
    return Math.max(max, stringWidth(row[columnIndex]))
  }, minWidth)
}
