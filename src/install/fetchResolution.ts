import {fetchFromRemoteTarball, FetchOptions} from '../resolve/fetch'
import {ResolveResult} from '../resolve'
import logger from 'pnpm-logger'
import execa = require('execa')

const gitLogger = logger('git')

export default async function fetchRes (res: ResolveResult, target: string, opts: FetchOptions): Promise<void> {
  if (res.tarball) {
    return fetchFromRemoteTarball(target, {
        shasum: res.shasum,
        tarball: res.tarball
      }, opts)
  }
  if (res.repo && res.commitId) {
      return clone(res.repo, res.commitId, target)
  }
  if (res.fetch) {
      return res.fetch(target)
  }
}

/**
 * clone a git repository.
 */
async function clone (repo: string, commitId: string, dest: string) {
  await execGit(['clone', repo, dest])
  await execGit(['checkout', commitId], {cwd: dest})
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function execGit (args: string[], opts?: Object) {
  gitLogger.debug(`executing git with args ${args}`)
  const fullArgs = prefixGitArgs().concat(args || [])
  return execa('git', fullArgs, opts)
}
