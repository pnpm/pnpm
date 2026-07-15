import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import { sanitizeForTerminal } from '@pnpm/deps.compliance.license-checker'
import type { LicensePackage } from '@pnpm/deps.compliance.license-scanner'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import { groupBy, omit, pick, sortWith } from 'ramda'
import semver from 'semver'

import type { LicensesCommandResult } from './LicensesCommandResult.js'

function sortLicensesPackages (licensePackages: readonly LicensePackage[]): LicensePackage[] {
  return sortWith(
    [
      (o1: LicensePackage, o2: LicensePackage) =>
        o1.license.localeCompare(o2.license),
    ],
    licensePackages
  )
}

function renderPackageName ({ belongsTo, name: packageName }: LicensePackage): string {
  const sanitizedName = sanitizeForTerminal(packageName as string)
  switch (belongsTo) {
    case 'devDependencies':
      return `${sanitizedName} ${chalk.dim('(dev)')}`
    case 'optionalDependencies':
      return `${sanitizedName} ${chalk.dim('(optional)')}`
    default:
      return sanitizedName
  }
}

function renderPackageLicense ({ license }: LicensePackage): string {
  const output = license ?? 'Unknown'
  return sanitizeForTerminal(output as string)
}

export function renderDetails (licensePackage: LicensePackage): string {
  // Sanitize each field individually before joining. The joined string is later
  // split on '\n' by renderLicensesTable to size the table rows, so the '\n'
  // separators must survive — sanitizing the joined string would strip them
  // (newline is a control char), collapsing multi-field details into one line.
  const outputs = []
  if (licensePackage.author) {
    outputs.push(sanitizeForTerminal(licensePackage.author))
  }
  if (licensePackage.description) {
    outputs.push(sanitizeForTerminal(licensePackage.description))
  }
  if (licensePackage.homepage) {
    outputs.push(sanitizeForTerminal(licensePackage.homepage))
  }
  return outputs.join('\n')
}

export function renderLicences (
  licensesMap: LicensePackage[],
  opts: { long?: boolean, json?: boolean }
): LicensesCommandResult {
  if (opts.json) {
    return { output: renderLicensesJson(licensesMap), exitCode: 0 }
  }

  return { output: renderLicensesTable(licensesMap, opts), exitCode: 0 }
}

function renderLicensesJson (licensePackages: readonly LicensePackage[]): string {
  const data = licensePackages
    .map((item) => pick(['name', 'version', 'path', 'license', 'author', 'homepage', 'description'], item))

  const output: Record<string, LicensePackageJson[]> = {}
  const groupedByLicense = groupBy((item) => item.license, data)
  for (const license in groupedByLicense) {
    const outputList: LicensePackageJson[] = []
    const groupedByName = groupBy((item) => item.name, groupedByLicense[license] ?? [])
    for (const inputList of Object.values(groupedByName)) {
      if (inputList == null) continue
      inputList.sort((a, b) => semver.compare(a.version, b.version))
      const versions = inputList.map((item) => item.version)
      const paths = inputList.map((item) => item.path ?? null)
      const lastInputItem = inputList.at(-1)! // last item is chosen for its latest information
      const outputItem: LicensePackageJson = {
        name: lastInputItem.name,
        versions,
        paths,
        ...omit(['name', 'version', 'path'], lastInputItem),
      }
      outputList.push(outputItem)
    }
    output[license] = outputList
  }

  return JSON.stringify(output, null, 2)
}

export interface LicensePackageJson {
  name: string
  versions: string[]
  license: string
  author?: string
  homepage?: string
  paths: Array<string | null>
}

function renderLicensesTable (
  licensePackages: readonly LicensePackage[],
  opts: { long?: boolean }
): string {
  const columnNames = ['Package', 'License']

  const columnFns = [renderPackageName, renderPackageLicense]

  if (opts.long) {
    columnNames.push('Details')
    columnFns.push(renderDetails)
  }

  // Avoid the overhead of allocating a new array caused by calling `array.map()`
  for (let i = 0; i < columnNames.length; i++)
    columnNames[i] = chalk.blueBright(columnNames[i])

  const data = [
    columnNames,
    ...deduplicateLicensesPackages(sortLicensesPackages(licensePackages))
      .map((licensePkg) => columnFns.map((fn) => fn(licensePkg))),
  ]
  let detailsColumnMaxWidth = 40
  let packageColumnMaxWidth = 0
  let licenseColumnMaxWidth = 0
  if (opts.long) {
    // Use the package link to determine the width of the details column
    detailsColumnMaxWidth = licensePackages.reduce((max, pkg) => Math.max(max, pkg.homepage?.length ?? 0), 0)
    for (let i = 1; i < data.length; i++) {
      const row = data[i]
      const detailsLineCount = row[2].split('\n').length
      const linesNumber = Math.max(0, detailsLineCount - 1)
      row[0] += '\n '.repeat(linesNumber) // Add extra spaces to the package column
      row[1] += '\n '.repeat(linesNumber) // Add extra spaces to the license column
      packageColumnMaxWidth = Math.max(packageColumnMaxWidth, row[0].length)
      licenseColumnMaxWidth = Math.max(licenseColumnMaxWidth, row[1].length)
    }
    const remainColumnWidth = process.stdout.columns - packageColumnMaxWidth - licenseColumnMaxWidth - 20
    if (detailsColumnMaxWidth > remainColumnWidth) {
      detailsColumnMaxWidth = remainColumnWidth
    }
    detailsColumnMaxWidth = Math.max(detailsColumnMaxWidth, 40)
  }
  try {
    return table(
      data,
      {
        ...TABLE_OPTIONS,
        columns: {
          ...TABLE_OPTIONS.columns,
          2: {
            width: detailsColumnMaxWidth,
            wrapWord: true,
          },
        },
      }
    )
  } catch {
    // Fallback to the default table if the details column width is too large, avoiding the error
    return table(
      data,
      TABLE_OPTIONS
    )
  }
}

function deduplicateLicensesPackages (licensePackages: LicensePackage[]): LicensePackage[] {
  const result: LicensePackage[] = []
  const rowEqual = (a: LicensePackage, b: LicensePackage) => a.name === b.name && a.license === b.license
  const hasRow = (row: LicensePackage) => result.some((x) => rowEqual(row, x))
  for (const row of licensePackages.reverse()) { // reverse + unshift to prioritize latest package description
    if (!hasRow(row)) result.unshift(row)
  }
  return result
}
