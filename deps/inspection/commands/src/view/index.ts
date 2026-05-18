import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { formatTimeAgo } from '@pnpm/resolving.npm-resolver'
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
    description: 'View package information from the registry without using npm CLI.',
    usages: [
      'pnpm view <package-name>',
      'pnpm view <package-name>@<version>',
      'pnpm view <package-name> [<field>[.subfield]...]',
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

export async function handler (
  opts: Config & ConfigContext & {
    json?: boolean
  },
  params: string[]
): Promise<string | void> {
  const packageSpec = params[0]

  if (!packageSpec) {
    throw new PnpmError('MISSING_PACKAGE_NAME', 'Package name is required. Usage: pnpm view <package-name>')
  }

  const fields = params.slice(1)

  const info = await fetchPackageInfo(opts, packageSpec)

  // If fields are specified, filter and return only those
  if (fields.length > 0) {
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
      const value = selectedFields[fields[0]]
      return formatFieldValue(value)
    }

    const lines = fields.map(field => {
      const value = selectedFields[field]
      if (typeof value === 'object' && value !== null) {
        return `${field} = ${JSON.stringify(value)}`
      }
      if (typeof value === 'string') {
        return `${field} = '${value}'`
      }
      return `${field} = ${formatFieldValue(value)}`
    })
    return lines.join('\n')
  }

  if (opts.json) {
    return JSON.stringify(info, null, 2)
  }

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

  const lines = [headerParts.join(' | ')]

  if (info.description) {
    lines.push(info.description)
  }

  if (info.homepage) {
    lines.push(chalk.underline.blue(info.homepage))
  }

  if (info.keywords && info.keywords.length > 0) {
    lines.push('')
    lines.push(`keywords: ${chalk.cyan(info.keywords.join(', '))}`)
  }

  if (info.dist) {
    lines.push('')
    lines.push(chalk.bold('dist'))
    if (info.dist.tarball) {
      lines.push(`.tarball: ${chalk.underline.blue(info.dist.tarball)}`)
    }
    if (info.dist.shasum) {
      lines.push(`.shasum: ${chalk.green(info.dist.shasum)}`)
    }
    if (info.dist.integrity) {
      lines.push(`.integrity: ${chalk.green(info.dist.integrity)}`)
    }
    if (info.dist.unpackedSize != null) {
      lines.push(`.unpackedSize: ${chalk.blue(formatBytes(info.dist.unpackedSize))}`)
    }
  }

  if (info.dependencies && Object.keys(info.dependencies).length > 0) {
    lines.push('')
    lines.push('dependencies:')
    const depEntries = Object.entries(info.dependencies).map(([name, version]) => `${chalk.blue(name)}: ${version}`)
    lines.push(depEntries.join(', '))
  }

  if (info.maintainers && info.maintainers.length > 0) {
    lines.push('')
    lines.push('maintainers:')
    for (const maintainer of info.maintainers) {
      const email = maintainer.email
      const name = email ? `${chalk.blue(maintainer.name)} <${chalk.dim(email)}>` : chalk.blue(maintainer.name)
      lines.push(`- ${name}`)
    }
  }

  if (info.distTags && Object.keys(info.distTags).length > 0) {
    lines.push('')
    lines.push(chalk.bold('dist-tags:'))
    for (const [tag, tagVersion] of Object.entries(info.distTags)) {
      lines.push(`${chalk.blue(tag)}: ${tagVersion}`)
    }
  }

  const publishedInfo = getPublishedInfo(info)
  if (publishedInfo) {
    lines.push('')
    lines.push(publishedInfo)
  }

  return lines.join('\n')
}

function formatBytes (bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'kB', 'MB', 'GB', 'TB', 'PB']
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1)
  return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i]
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
