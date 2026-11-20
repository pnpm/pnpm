import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import { compareVersions, type LicensePackage } from '@pnpm/deps.compliance.license-scanner'
import { sanitizeInline } from '@pnpm/text.sanitize'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import { groupBy, omit, pick, sortWith } from 'ramda'

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

function renderPackageName ({ belongsTo, name, registryName }: LicensePackage): string {
  // The alias disambiguates two otherwise identical rows for the same
  // package served by different registries.
  const packageName = registryName == null ? sanitizeInline(name) : `${sanitizeInline(name)} ${chalk.dim(`(${sanitizeInline(registryName)})`)}`
  switch (belongsTo) {
    case 'devDependencies':
      return `${packageName} ${chalk.dim('(dev)')}`
    case 'optionalDependencies':
      return `${packageName} ${chalk.dim('(optional)')}`
    default:
      return packageName as string
  }
}

function renderPackageLicense ({ license }: LicensePackage): string {
  const output = sanitizeInline(license ?? 'Unknown')
  return output as string
}

function renderDetails (licensePackage: LicensePackage): string {
  const outputs = []
  if (licensePackage.author) {
    outputs.push(licensePackage.author)
  }
  if (licensePackage.description) {
    outputs.push(licensePackage.description)
  }
  if (licensePackage.homepage) {
    outputs.push(licensePackage.homepage)
  }
  return outputs.map((value) => value.split('\n').map(sanitizeInline).join('\n')).join('\n')
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
    .map((item) => pick(['name', 'version', 'path', 'paths', 'license', 'author', 'homepage', 'description', 'registryName'], item))

  const output: Record<string, LicensePackageJson[]> = {}
  const groupedByLicense = groupBy((item) => item.license, data)
  for (const license in groupedByLicense) {
    // Group by the registry too: the same name from two registries is two
    // packages, and collapsing them would drop one from the report.
    const groupedByName = groupBy(
      (item) => item.registryName == null ? item.name : `${item.name}\u0000${item.registryName}`,
      groupedByLicense[license] ?? []
    )
    output[license] = Object.values(groupedByName)
      .filter((inputList) => inputList != null)
      .map(mergeLicensePackageVersions)
  }

  return JSON.stringify(output, null, 2)
}

type PickedLicensePackage = Pick<LicensePackage, 'name' | 'version' | 'path' | 'paths' | 'license' | 'author' | 'homepage' | 'description' | 'registryName'>

function mergeLicensePackageVersions (inputList: PickedLicensePackage[]): LicensePackageJson {
  inputList.sort((a, b) => compareVersions(a.version, b.version))
  const versions = inputList.map((item) => item.version)
  const paths = [...new Set(inputList.flatMap((item) => item.paths ?? [item.path ?? null]))]
  const lastInputItem = inputList.at(-1)! // last item is chosen for its latest information
  return {
    name: lastInputItem.name,
    versions,
    paths,
    ...omit(['name', 'version', 'path', 'paths'], lastInputItem),
  }
}

export interface LicensePackageJson {
  name: string
  versions: string[]
  /** Named-registry alias the package came from, when it is not the default registry. */
  registryName?: string
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
  for (let columnIndex = 0; columnIndex < columnNames.length; columnIndex++)
    columnNames[columnIndex] = chalk.blueBright(columnNames[columnIndex])

  const data = [
    columnNames,
    ...deduplicateLicensesPackages(sortLicensesPackages(licensePackages))
      .map((licensePkg) => columnFns.map((fn) => fn(licensePkg))),
  ]
  const detailsColumnMaxWidth = opts.long ? fitDetailsColumn(data, licensePackages) : 40
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

/**
 * Pads the package and license cells of each row to the height of its details
 * cell and returns the width the details column can take.
 */
function fitDetailsColumn (data: string[][], licensePackages: readonly LicensePackage[]): number {
  // Use the package link to determine the width of the details column
  let detailsColumnMaxWidth = licensePackages.reduce((max, pkg) => Math.max(max, pkg.homepage?.length ?? 0), 0)
  let packageColumnMaxWidth = 0
  let licenseColumnMaxWidth = 0
  for (let rowIndex = 1; rowIndex < data.length; rowIndex++) {
    const row = data[rowIndex]
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
  return Math.max(detailsColumnMaxWidth, 40)
}

function deduplicateLicensesPackages (licensePackages: LicensePackage[]): LicensePackage[] {
  const result: LicensePackage[] = []
  const rowEqual = (left: LicensePackage, right: LicensePackage) =>
    left.name === right.name && left.license === right.license && left.registryName === right.registryName
  const hasRow = (row: LicensePackage) => result.some((existingRow) => rowEqual(row, existingRow))
  for (const row of licensePackages.reverse()) { // reverse + unshift to prioritize latest package description
    if (!hasRow(row)) result.unshift(row)
  }
  return result
}
