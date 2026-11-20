import path from 'node:path'

import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import { formatTimeAgo } from '@pnpm/resolving.npm-resolver'
import type { ProjectManifest } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import chalk from 'chalk'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { type ExtendedPackageInfo, fetchPackageInfo } from '../fetchPackageInfo.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(['registry'], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    json: Boolean,
  }
}

export const commandNames = ['view', 'info', 'show', 'v']

export function help (): string {
  return renderHelp({
    description: 'View package information from the registry. If package name is omitted, searches upward for the nearest package manifest.',
    usages: [
      'pnpm view [<package-name>]',
      'pnpm view [<package-name>@<version>]',
      'pnpm view [<package-name>] [<field>[.subfield]...]',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
        ],
      },
    ],
  })
}

type ViewCommandOptions = Config & ConfigContext & {
  json?: boolean
}

export async function handler (
  opts: ViewCommandOptions,
  params: string[]
): Promise<string | void> {
  const packageSpec = params[0] || await readNearestProjectName(opts)
  const fields = params.slice(1)

  const info = await fetchPackageInfo(opts, packageSpec)

  // If fields are specified, filter and return only those
  if (fields.length > 0) {
    return renderSelectedFields(info, fields, opts)
  }

  if (opts.json) {
    return JSON.stringify(info, null, 2)
  }

  return PACKAGE_INFO_SECTIONS.flatMap((renderSection) => renderSection(info)).join('\n')
}

async function readNearestProjectName (opts: ViewCommandOptions): Promise<string> {
  const nearestManifest = await findNearestProjectManifest(opts.dir ?? process.cwd(), opts)
  if (!nearestManifest) {
    throw new PnpmError('MISSING_PACKAGE_NAME', 'Package name is required. Usage: pnpm view [<package-name>]')
  }
  if (typeof nearestManifest.manifest.name !== 'string' || nearestManifest.manifest.name.length === 0) {
    throw new PnpmError(
      'INVALID_PACKAGE_JSON',
      `Invalid ${nearestManifest.fileName} at "${nearestManifest.projectDir}". The "name" field is required and must be a non-empty string.`
    )
  }
  return nearestManifest.manifest.name
}

function renderSelectedFields (info: ExtendedPackageInfo, fields: string[], opts: { json?: boolean }): string {
  const selectedFields: Record<string, unknown> = {}
  for (const field of fields) {
    selectedFields[field] = getNestedProperty(info as unknown as Record<string, unknown>, field)
  }

  if (opts.json) {
    if (fields.length === 1) {
      return JSON.stringify(selectedFields[fields[0]], null, 2)
    }
    return JSON.stringify(selectedFields, null, 2)
  }

  if (fields.length === 1) {
    return formatFieldValue(selectedFields[fields[0]])
  }

  return fields.map((field) => formatFieldAssignment(field, selectedFields[field])).join('\n')
}

function formatFieldAssignment (field: string, value: unknown): string {
  if (typeof value === 'object' && value !== null) {
    return `${field} = ${JSON.stringify(value)}`
  }
  if (typeof value === 'string') {
    return `${field} = '${value}'`
  }
  return `${field} = ${formatFieldValue(value)}`
}

const PACKAGE_INFO_SECTIONS: Array<(info: ExtendedPackageInfo) => string[]> = [
  renderHeader,
  renderDescriptionAndHomepage,
  renderDeprecation,
  renderKeywords,
  renderBins,
  renderDist,
  renderDependencies,
  renderMaintainers,
  renderDistTags,
  renderPublishedInfo,
]

function renderHeader (info: ExtendedPackageInfo): string[] {
  const headerParts: string[] = []

  if (info.name && info.version) {
    headerParts.push(chalk.cyan(`${info.name}@${info.version}`))
  }

  if (info.license) {
    headerParts.push(chalk.green(info.license))
  }

  if (info.depsCount !== undefined) {
    headerParts.push(`deps: ${chalk.cyan(info.depsCount)}`)
  } else {
    headerParts.push('deps: none')
  }

  if (info.versionsCount !== undefined) {
    headerParts.push(`versions: ${chalk.cyan(info.versionsCount)}`)
  }

  return [headerParts.join(' | ')]
}

function renderDescriptionAndHomepage (info: ExtendedPackageInfo): string[] {
  const lines: string[] = []
  if (info.description) {
    lines.push(info.description)
  }
  if (info.homepage) {
    lines.push(chalk.underline.blue(info.homepage))
  }
  return lines
}

function renderDeprecation (info: ExtendedPackageInfo): string[] {
  if (!info.deprecated) return []
  return ['', `${chalk.red('DEPRECATED!')} - ${info.deprecated}`]
}

function renderKeywords (info: ExtendedPackageInfo): string[] {
  if (!info.keywords || info.keywords.length === 0) return []
  return ['', `keywords: ${chalk.cyan(info.keywords.join(', '))}`]
}

function renderBins (info: ExtendedPackageInfo): string[] {
  if (!info.bin) return []
  const bins = listBinNames(info.bin, info.name)
  if (bins.length === 0) return []
  return ['', `bin: ${chalk.cyan(bins.join(', '))}`]
}

function listBinNames (bin: NonNullable<ExtendedPackageInfo['bin']>, pkgName: string | undefined): string[] {
  if (typeof bin !== 'string') return Object.keys(bin)
  if (bin.length === 0 || !pkgName) return []
  return [pkgName[0] === '@' ? pkgName.slice(pkgName.indexOf('/') + 1) : pkgName]
}

function renderDist ({ dist }: ExtendedPackageInfo): string[] {
  if (!dist) return []
  const lines = ['', chalk.bold('dist')]
  if (dist.tarball) {
    lines.push(`.tarball: ${chalk.underline.blue(dist.tarball)}`)
  }
  if (dist.shasum) {
    lines.push(`.shasum: ${chalk.green(dist.shasum)}`)
  }
  if (dist.integrity) {
    lines.push(`.integrity: ${chalk.green(dist.integrity)}`)
  }
  if (dist.unpackedSize != null) {
    lines.push(`.unpackedSize: ${chalk.blue(formatBytes(dist.unpackedSize))}`)
  }
  return lines
}

function renderDependencies (info: ExtendedPackageInfo): string[] {
  if (!info.dependencies || Object.keys(info.dependencies).length === 0) return []
  const depEntries = Object.entries(info.dependencies).map(([name, version]) => `${chalk.blue(name)}: ${version}`)
  return ['', 'dependencies:', depEntries.join(', ')]
}

function renderMaintainers (info: ExtendedPackageInfo): string[] {
  if (!info.maintainers || info.maintainers.length === 0) return []
  return [
    '',
    'maintainers:',
    ...info.maintainers.map((maintainer) => `- ${formatPerson(maintainer)}`),
  ]
}

function renderDistTags (info: ExtendedPackageInfo): string[] {
  if (!info.distTags || Object.keys(info.distTags).length === 0) return []
  return [
    '',
    chalk.bold('dist-tags:'),
    ...Object.entries(info.distTags).map(([tag, tagVersion]) => `${chalk.blue(tag)}: ${tagVersion}`),
  ]
}

function renderPublishedInfo (info: ExtendedPackageInfo): string[] {
  const publishedInfo = getPublishedInfo(info)
  if (!publishedInfo) return []
  return ['', publishedInfo]
}

function formatPerson ({ name, email }: { name: string, email?: string }): string {
  return email ? `${chalk.blue(name)} <${chalk.dim(email)}>` : chalk.blue(name)
}

function formatBytes (bytes: number): string {
  if (bytes === 0) return '0 B'
  const unitBase = 1000
  const sizes = ['B', 'kB', 'MB', 'GB', 'TB', 'PB']
  const unitIndex = Math.min(Math.floor(Math.log(bytes) / Math.log(unitBase)), sizes.length - 1)
  return Math.round((bytes / Math.pow(unitBase, unitIndex)) * 100) / 100 + ' ' + sizes[unitIndex]
}

function getNestedProperty (obj: Record<string, unknown>, path: string): unknown {
  return path.split('.').reduce((acc: unknown, part) => {
    if (typeof acc === 'object' && acc !== null) {
      return (acc as Record<string, unknown>)[part]
    }
    return undefined
  }, obj)
}

function formatFieldValue (value: unknown): string {
  if (value === null || value === undefined) {
    return ''
  }
  if (typeof value === 'object') {
    return JSON.stringify(value, null, 2)
  }
  return String(value)
}

function getPublishedInfo (info: ExtendedPackageInfo): string | null {
  if (!info.version || !info.time) {
    return null
  }
  const publishedTime = info.time[info.version]
  if (!publishedTime) {
    return null
  }
  const publishedDate = new Date(publishedTime)
  if (isNaN(publishedDate.getTime())) {
    return null
  }
  const timeAgo = formatTimeAgo(publishedDate) ?? 'just now'

  const publisher = getPublisher(info)
  if (publisher) {
    return `published ${chalk.cyan(timeAgo)} by ${publisher}`
  }
  return `published ${chalk.cyan(timeAgo)}`
}

async function findNearestProjectManifest (
  startDir: string,
  _opts: Config & ConfigContext
): Promise<{ manifest: ProjectManifest, fileName: string, projectDir: string } | null> {
  try {
    const result = await tryReadProjectManifest(startDir)
    if (result.manifest != null) {
      return {
        manifest: result.manifest,
        fileName: result.fileName,
        projectDir: startDir,
      }
    }
  } catch (err: unknown) {
    const message = isError(err) ? err.message : String(err)
    throw new PnpmError('INVALID_PACKAGE_JSON', `Failed to read or parse project manifest in "${startDir}": ${message}`)
  }
  const parentDir = path.dirname(startDir)
  if (parentDir === startDir) {
    return null
  }
  return findNearestProjectManifest(parentDir, _opts)
}

/**
 * Retrieves the publisher name from package metadata.
 * Checks fields in order: _npmUser, maintainers, author.
 * Returns null if no publisher information is available.
 */
function getPublisher (info: ExtendedPackageInfo): string | null {
  if (info._npmUser?.name) {
    const email = info._npmUser.email
    return email ? `${chalk.blue(info._npmUser.name)} <${chalk.dim(email)}>` : chalk.blue(info._npmUser.name)
  }
  if (info.maintainers && info.maintainers.length > 0) {
    const first = info.maintainers[0]
    const email = first.email
    const name = email ? `${chalk.blue(first.name)} <${chalk.dim(email)}>` : chalk.blue(first.name)
    if (info.maintainers.length === 1) {
      return name
    }
    return `${name} et al.`
  }
  if (info.author) {
    return String(info.author)
  }
  return null
}
