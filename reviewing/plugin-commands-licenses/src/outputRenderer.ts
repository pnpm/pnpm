import { TABLE_OPTIONS } from '@pnpm/cli-utils'
import { LicensePackage } from '@pnpm/license-scanner'
import chalk from 'chalk'
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

function renderPackageName ({ belongsTo, name: packageName }: LicensePackage) {
  switch (belongsTo) {
  case 'devDependencies':
    return `${packageName} ${chalk.dim('(dev)')}`
  case 'optionalDependencies':
    return `${packageName} ${chalk.dim('(optional)')}`
  default:
    return packageName as string
  }
}

function renderPackageLicense ({ license }: LicensePackage) {
  const output = license ?? 'Unknown'
  return output as string
}

function renderDetails (licensePackage: LicensePackage) {
  const outputs = []
  if (licensePackage.author) {
    outputs.push(licensePackage.author)
  }
  if (licensePackage.homepage) {
    outputs.push(chalk.underline(licensePackage.homepage))
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
        name: licensePkg.name,
        version: licensePkg.version,
        path: licensePkg.path,
        license: licensePkg.license,
        licenseContents: licensePkg.licenseContents,
        author: licensePkg.author,
        homepage: licensePkg.homepage,
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
  author: string
  homepage: string
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
