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
    usages: ['pnpm bugs'],
  })
}

export async function handler (
  opts: {
    dir: string
  }
): Promise<void> {
  const { manifest } = await tryReadProjectManifest(opts.dir, {})
  let bugsUrl: string | undefined

  if (manifest?.bugs && typeof manifest.bugs !== 'string' && manifest.bugs.url) {
    bugsUrl = manifest.bugs.url
  } else if (typeof manifest?.bugs === 'string') {
    bugsUrl = manifest.bugs
  } else if (typeof manifest?.repository === 'object' && manifest.repository.url) {
    bugsUrl = `${manifest.repository.url.replace(/\.git$/, '')}/issues`
  } else if (typeof manifest?.repository === 'string') {
    const repoUrl = manifest.repository.replace(/\.git$/, '')
    bugsUrl = `${repoUrl}/issues`
  }

  if (!bugsUrl) {
    throw new PnpmError('NO_BUGS_URL', 'The package.json does not have a bugs URL. Add a "bugs" field or a "repository" field to your package.json.')
  }

  await openUrl(bugsUrl)
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
  await new Promise<void>((resolve, reject) => {
    execFile(cmd, args, (err) => {
      if (err) reject(err)
      else resolve()
    })
  })
}
