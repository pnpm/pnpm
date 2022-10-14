import path from 'path'
import type { GitFetcher } from '@pnpm/fetcher-base'
import { preparePackage } from '@pnpm/prepare-package'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import { URL } from 'url'

export function createGitFetcher (createOpts?: { gitShallowHosts?: string[] }) {
  const allowedHosts = new Set(createOpts?.gitShallowHosts ?? [])

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
    await preparePackage(tempLocation)
    // removing /.git to make directory integrity calculation faster
    await rimraf(path.join(tempLocation, '.git'))
    const filesIndex = await cafs.addFilesFromDir(tempLocation, opts.manifest)
    // Important! We cannot remove the temp location at this stage.
    // Even though we have the index of the package,
    // the linking of files to the store is in progress.
    return { filesIndex }
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
