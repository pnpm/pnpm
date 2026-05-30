import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import HostedGit from 'hosted-git-info'
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

export const commandNames = ['repo']

export function help (): string {
  return renderHelp({
    description: "Opens the URL of the package's repository in a browser.",
    url: docsUrl('repo'),
    usages: ['pnpm repo [<pkgname> [<pkgname> ...]]'],
  })
}

export async function handler (
  opts: Config & ConfigContext & { dir: string },
  params: string[]
): Promise<void> {
  const urls = params.length === 0
    ? [await getRepoUrlFromCurrentProject(opts)]
    : await Promise.all(params.map((spec) => getRepoUrlFromRegistry(opts, spec)))
  for (const url of urls) {
    // eslint-disable-next-line no-await-in-loop
    await open(url)
  }
}

async function getRepoUrlFromCurrentProject (
  opts: Pick<Config, 'dir' | 'engineStrict' | 'nodeVersion' | 'supportedArchitectures'>
): Promise<string> {
  const manifest = await readProjectManifestOnly(opts.dir, {
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    supportedArchitectures: opts.supportedArchitectures,
  })
  const url = pickRepoUrl(manifest.repository)
  if (!url) {
    throw new PnpmError(
      'NO_REPO_URL',
      'The current project does not have a repository URL. Add a "repository" field to its manifest.'
    )
  }
  return url
}

async function getRepoUrlFromRegistry (
  opts: Config & ConfigContext,
  packageSpec: string
): Promise<string> {
  const info = await fetchPackageInfo(opts, packageSpec)
  const url = pickRepoUrl(info.repository)
  if (!url) {
    throw new PnpmError('NO_REPO_URL', `The package "${info.name}" does not have a repository URL.`)
  }
  return url
}

function pickRepoUrl (
  repository: string | { url?: string, directory?: string } | undefined
): string | undefined {
  if (!repository) return undefined
  const repoUrl = typeof repository === 'string' ? repository : repository.url
  if (!repoUrl) return undefined
  const directory = typeof repository === 'object' ? repository.directory : undefined
  return repositoryToWebUrl(repoUrl, directory)
}

function repositoryToWebUrl (rawUrl: string, directory?: string): string | undefined {
  const hosted = HostedGit.fromUrl(rawUrl)
  if (hosted != null) {
    const url = directory ? hosted.browse(directory) : hosted.browse()
    if (url && isHttpUrl(url)) {
      return url
    }
  }
  let parsed: URL
  try {
    parsed = new URL(rawUrl.replace(/^git\+/, ''))
  } catch {
    return undefined
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined
  parsed.search = ''
  parsed.hash = ''
  parsed.pathname = parsed.pathname.replace(/\/+$/, '').replace(/\.git$/, '')
  if (directory) {
    parsed.pathname += `/tree/HEAD/${directory.replace(/^\//, '')}`
  }
  return parsed.toString()
}

function isHttpUrl (value: string): boolean {
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}
