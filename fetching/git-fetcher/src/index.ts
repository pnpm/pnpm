import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import type { GitFetcher } from '@pnpm/fetcher-base'
import { globalWarn } from '@pnpm/logger'
import { preparePackage } from '@pnpm/prepare-package'
import { addFilesFromDir } from '@pnpm/worker'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import { URL } from 'url'

export interface CreateGitFetcherOptions {
  gitShallowHosts?: string[]
  rawConfig: object
  unsafePerm?: boolean
  ignoreScripts?: boolean
}

export function createGitFetcher (createOpts: CreateGitFetcherOptions) {
  const allowedHosts = new Set(createOpts?.gitShallowHosts ?? [])
  const ignoreScripts = createOpts.ignoreScripts ?? false
  const preparePkg = preparePackage.bind(null, {
    ignoreScripts: createOpts.ignoreScripts,
    rawConfig: createOpts.rawConfig,
    unsafePerm: createOpts.unsafePerm,
  })

  const gitFetcher: GitFetcher = async (cafs, resolution, opts) => {
    const tempLocation = await cafs.tempDir()
    if (allowedHosts.size > 0 && shouldUseShallow(resolution.repo, allowedHosts)) {
      await execGit(['init'], { cwd: tempLocation })
      await execGit(['remote', 'add', 'origin', resolution.repo], { cwd: tempLocation })
      await execGit(['fetch', '--depth', '1', 'origin', resolution.commit], { cwd: tempLocation })
    } else {
      await execGit(['clone', resolution.repo, tempLocation])
    }
    await execGit(['checkout', resolution.commit], { cwd: tempLocation })
    try {
      const shouldBeBuilt = await preparePkg(tempLocation, resolution.path ?? tempLocation)
      if (ignoreScripts && shouldBeBuilt) {
        globalWarn(`The git-hosted package fetched from "${resolution.repo}" has to be built but the build scripts were ignored.`)
      }
    } catch (err: any) { // eslint-disable-line
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.repo}": ${err.message}`
      throw err
    }
    // removing /.git to make directory integrity calculation faster
    await rimraf(path.join(tempLocation, '.git'))
    // Important! We cannot remove the temp location at this stage.
    // Even though we have the index of the package,
    // the linking of files to the store is in progress.
    return addFilesFromDir({
      cafsDir: cafs.cafsDir,
      dir: resolution.path
        ? getJoinedPath(tempLocation, resolution.path, resolution.repo)
        : tempLocation,
      filesIndexFile: opts.filesIndexFile,
      readManifest: opts.readManifest,
      pkg: opts.pkg,
    })
  }

  return {
    git: gitFetcher,
  }
}

function shouldUseShallow (repoUrl: string, allowedHosts: Set<string>): boolean {
  try {
    const { host } = new URL(repoUrl)
    if (allowedHosts.has(host)) {
      return true
    }
  } catch (e) {
    // URL might be malformed
  }
  return false
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function execGit (args: string[], opts?: object) {
  const fullArgs = prefixGitArgs().concat(args || [])
  return execa('git', fullArgs, opts)
}

function getJoinedPath (root: string, sub: string, repo: string) {
  const joined = path.join(root, sub)
  // prevent the dir traversal attack
  const relative = path.relative(root, joined)
  if (relative.startsWith('..')) {
    throw new PnpmError('INVALID_PATH', `Path "${sub}" should be a sub directory in Git repository "${repo}"`)
  }
  if (!fs.existsSync(joined) || !fs.lstatSync(joined).isDirectory()) {
    throw new PnpmError('INVALID_PATH', `Path "${sub}" is not a directory in Git repository "${repo}"`)
  }
  return joined
}
