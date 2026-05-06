import { docsUrl, tryReadProjectManifest } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import open from 'open'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { fetchPackageInfo } from '../fetchPackageInfo.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(['registry'], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return rcOptionsTypes()
}

export const commandNames = ['bugs']

export function help (): string {
  return renderHelp({
    description: "Opens the URL of the package's bug tracker in a browser.",
    url: docsUrl('bugs'),
    usages: ['pnpm bugs [<pkgname> [<pkgname> ...]]'],
  })
}

export async function handler (
  opts: Config & ConfigContext & { dir: string },
  params: string[]
): Promise<void> {
  const urls = params.length === 0
    ? [await getBugsUrlFromCurrentProject(opts.dir)]
    : await Promise.all(params.map((spec) => getBugsUrlFromRegistry(opts, spec)))
  for (const url of urls) {
    // eslint-disable-next-line no-await-in-loop
    await open(url)
  }
}

async function getBugsUrlFromCurrentProject (dir: string): Promise<string> {
  const { manifest } = await tryReadProjectManifest(dir, {})
  if (manifest == null) {
    throw new PnpmError('NO_MANIFEST_FOUND', `No package.json was found in "${dir}"`)
  }
  const url = pickBugsUrl({ bugs: manifest.bugs, repository: manifest.repository })
  if (!url) {
    throw new PnpmError(
      'NO_BUGS_URL',
      'The package.json does not have a bug tracker URL. Add a "bugs" or "repository" field to your package.json.'
    )
  }
  return url
}

async function getBugsUrlFromRegistry (
  opts: Config & ConfigContext,
  packageSpec: string
): Promise<string> {
  const info = await fetchPackageInfo(opts, packageSpec)
  const url = pickBugsUrl({ bugs: info.bugs, repository: info.repository })
  if (!url) {
    throw new PnpmError('NO_BUGS_URL', `The package "${info.name}" does not have a bug tracker URL.`)
  }
  return url
}

function pickBugsUrl (
  manifest: { bugs?: string | { url?: string }, repository?: string | { url?: string } }
): string | undefined {
  if (manifest.bugs) {
    const bugsUrl = typeof manifest.bugs === 'string' ? manifest.bugs : manifest.bugs.url
    if (bugsUrl && isHttpUrl(bugsUrl)) return bugsUrl
  }
  if (manifest.repository) {
    const repoUrl = typeof manifest.repository === 'string' ? manifest.repository : manifest.repository.url
    if (repoUrl) {
      const normalized = normalizeRepositoryUrl(repoUrl)
      if (normalized && isHttpUrl(normalized)) return `${normalized}/issues`
    }
  }
  return undefined
}

function normalizeRepositoryUrl (url: string): string | undefined {
  let normalized = url.replace(/^git\+/, '').replace(/\.git$/, '').replace(/\/+$/, '')
  if (normalized.startsWith('git://')) {
    normalized = `https://${normalized.slice('git://'.length)}`
  } else if (normalized.startsWith('git@github.com:')) {
    normalized = `https://github.com/${normalized.slice('git@github.com:'.length)}`
  }
  return normalized
}

function isHttpUrl (value: string): boolean {
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}
