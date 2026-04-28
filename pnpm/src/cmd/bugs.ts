import { docsUrl } from '@pnpm/cli.utils'
import { tryReadProjectManifest } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { renderHelp } from 'render-help'

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export const rcOptionsTypes = cliOptionsTypes

export const commandNames = ['bugs']

export function help (): string {
  return renderHelp({
    description: 'Opens the URL of the package bug tracker in the browser.',
    descriptionLists: [
      {
        title: 'Options',

        list: [],
      },
    ],
    url: docsUrl('bugs'),
    usages: ['pnpm bugs', 'pnpm bugs [<pkgname> [<pkgname> ...]]'],
  })
}

export async function handler (
  opts: {
    dir: string
    registries?: {
      default?: string
      [key: string]: string | undefined
    }
  },
  params: string[]
): Promise<void> {
  const pkgNames = params

  if (pkgNames.length === 0) {
    const { manifest } = await tryReadProjectManifest(opts.dir, {})
    const bugsUrl = getBugsUrlFromManifest(manifest)

    if (!bugsUrl) {
      throw new PnpmError('NO_BUGS_URL', 'The package.json does not have a bugs URL. Add a "bugs" field or a "repository" field to your package.json.')
    }

    await openUrl(bugsUrl)
  } else {
    const getBugsUrlFromRegistry = async (pkgName: string): Promise<string> => {
      const registry = opts.registries?.default ?? 'https://registry.npmjs.org'
      const url = `${registry.replace(/\/$/, '')}/${pkgName}`
      const response = await fetch(url, {
        headers: {
          'accept': 'application/json',
        },
      })
      if (!response.ok) {
        throw new PnpmError('PACKAGE_NOT_FOUND', `Could not fetch package "${pkgName}" from registry: ${response.status} ${response.statusText}`)
      }
      const data: Record<string, unknown> = await response.json()
      const bugsUrl = getBugsUrlFromManifest(data as PackageManifest)
      if (!bugsUrl) {
        throw new PnpmError('NO_BUGS_URL', `The package "${pkgName}" does not have a bugs URL.`)
      }
      return bugsUrl
    }

    const bugsUrls = await Promise.all(pkgNames.map(getBugsUrlFromRegistry))
    await Promise.all(bugsUrls.map(openUrl))
  }
}

type PackageManifest = {
  bugs?: { url?: string } | string
  repository?: { url?: string } | string
} | null

function getBugsUrlFromManifest (manifest: PackageManifest): string | undefined {
  if (manifest?.bugs && typeof manifest.bugs !== 'string' && manifest.bugs.url) {
    return manifest.bugs.url
  }
  if (typeof manifest?.bugs === 'string') {
    return manifest.bugs
  }
  if (typeof manifest?.repository === 'object' && manifest.repository.url) {
    return `${manifest.repository.url.replace(/\.git$/, '')}/issues`
  }
  if (typeof manifest?.repository === 'string') {
    const repoUrl = manifest.repository.replace(/\.git$/, '')
    return `${repoUrl}/issues`
  }
  return undefined
}

async function openUrl (url: string): Promise<void> {
  let parsedUrl: URL
  try {
    parsedUrl = new URL(url)
  } catch {
    throw new PnpmError('INVALID_BUGS_URL', `The bugs URL "${url}" is invalid`)
  }
  if (parsedUrl.protocol !== 'https:' && parsedUrl.protocol !== 'http:') {
    throw new PnpmError('INVALID_BUGS_URL', `The bugs URL "${url}" must use http or https protocol`)
  }

  const canonicalUrl = parsedUrl.href
  const { platform } = await import('node:process')
  let cmd: string
  let args: string[]

  switch (platform) {
    case 'darwin':
      cmd = 'open'
      args = [canonicalUrl]
      break
    case 'win32': {
      cmd = 'cmd'
      const escapedUrl = canonicalUrl.replace(/[&|<>^%()!]/g, '^$&')
      args = ['/c', 'start', '', escapedUrl]
      break
    }
    default:
      cmd = 'xdg-open'
      args = [canonicalUrl]
      break
  }

  const { execFile } = await import('node:child_process')
  await new Promise<void>((resolve) => {
    execFile(cmd, args, (err) => {
      if (err) resolve()
      else resolve()
    })
  })
}

