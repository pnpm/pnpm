import { TABLE_OPTIONS } from '@pnpm/cli-utils'
import { LicensePackage } from '@pnpm/licenses'
import chalk from 'chalk'
import stripAnsi from 'strip-ansi'
import { table } from '@zkochan/table'
import { groupBy, sortWith } from 'ramda'

function sortLicensesPackages (licensePackages: readonly LicensePackage[]) {
  return sortWith(
    [
      (o1: LicensePackage, o2: LicensePackage) =>
        o1.license.localeCompare(o2.license),
    ],
    licensePackages
  )
}

export function getCellWidth (
  data: string[][],
  columnNumber: number,
  maxWidth: number
) {
  const maxCellWidth = data.reduce((cellWidth, row) => {
    const cellLines = stripAnsi(row[columnNumber])?.split('\n') ?? []
    const currentCellWidth = cellLines.reduce((lineWidth, line) => {
      return Math.max(lineWidth, line.length)
    }, 0)
    return Math.max(cellWidth, currentCellWidth)
  }, 0)
  return Math.min(maxWidth, maxCellWidth)
}

export function renderPackageName ({ belongsTo, packageName }: LicensePackage) {
  switch (belongsTo) {
  case 'devDependencies':
    return `${packageName} ${chalk.dim('(dev)')}`
  case 'optionalDependencies':
    return `${packageName} ${chalk.dim('(optional)')}`
  default:
    return packageName as string
  }
}

export function renderPackageLicense ({ license }: LicensePackage) {
  const output = license ?? 'Unknown'
  return output as string
}

export function renderDetails (licensePackage: LicensePackage) {
  const { packageManifest, author } = licensePackage
  if (packageManifest == null) return ''
  const outputs = []
  if (author) {
    outputs.push(author)
  }
  if (packageManifest.homepage) {
    outputs.push(chalk.underline(packageManifest.homepage))
  }
  return outputs.join('\n')
}
export function renderLicences (
  licensesMap: LicensePackage[],
  opts: { long?: boolean, json?: boolean }
) {
  if (opts.json) {
    return { output: renderLicensesJson(licensesMap), exitCode: 0 }
  }

  return { output: renderLicensesTable(licensesMap, opts), exitCode: 0 }
}

function renderLicensesJson (licensePackages: readonly LicensePackage[]) {
  const data = [
    ...licensePackages.map((licensePkg) => {
      return {
        name: licensePkg.packageName,
        version: licensePkg.version,
        path: licensePkg.packageDir,
        license: licensePkg.license,
        licenseContents: licensePkg.licenseContents,
        vendorName: licensePkg.author,
        vendorUrl: licensePkg.packageManifest?.homepage,
      } as LicensePackageJson
    }),
  ].flat()

  // Group the package by license
  const groupByLicense = groupBy((item: LicensePackageJson) => item.license)
  const groupedByLicense = groupByLicense(data)

  return JSON.stringify(groupedByLicense, null, 2)
}

export interface LicensePackageJson {
  name: string
  license: string
  vendorName: string
  vendorUrl: string
  path: string
}

function renderLicensesTable (
  licensePackages: readonly LicensePackage[],
  opts: { long?: boolean }
) {
  const columnNames = ['Package', 'License']

  const columnFns = [renderPackageName, renderPackageLicense]

  if (opts.long) {
    columnNames.push('Details')
    columnFns.push(renderDetails)
  }

  // Avoid the overhead of allocating a new array caused by calling `array.map()`
  for (let i = 0; i < columnNames.length; i++)
    columnNames[i] = chalk.blueBright(columnNames[i])

  return table(
    [
      columnNames,
      ...sortLicensesPackages(licensePackages).map((licensePkg) => {
        return columnFns.map((fn) => fn(licensePkg))
      }),
    ],
    TABLE_OPTIONS
  )
}
