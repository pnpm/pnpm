import {fetchFromRemoteTarball, FetchOptions} from '../resolve/fetch'
import {ResolveResult} from '../resolve'
import logger from '../logger'
import spawn = require('cross-spawn')

const gitLogger = logger('git')

export default async function fetchRes (res: ResolveResult, target: string, opts: FetchOptions): Promise<void> {
  if (res.tarball) {
    return fetchFromRemoteTarball(target, {
        shasum: res.shasum,
        tarball: res.tarball
      }, opts)
  }
  if (res.repo && res.ref) {
      return clone(res.repo, res.ref, target)
  }
  if (res.fetch) {
      return res.fetch(target)
  }
}

/**
 * clone a git repository.
 */
export async function clone (repo: string, ref: string, dest: string) {
  await new Promise((resolve, reject) => {
    const args = ['clone', '-b', ref, repo, dest, '--single-branch']
    gitLogger.debug(`cloning git repository from ${repo}`)
    const git = spawnGit(args)
    let errMsg = ''
    git.stderr.on('data', (data: string) => errMsg += data)
    git.on('close', (code: number) => (code ? errorHandler() : resolve()))

    function errorHandler () {
      gitLogger.debug(`failed to clone repository from ${repo}`)
      reject(new Error(`failed to clone repository from ${repo}
        ${errMsg}`))
    }
  })
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function spawnGit (args: string[]) {
  gitLogger.debug(`executing git with args ${args}`)
  const fullArgs = prefixGitArgs().concat(args || [])
  return spawn('git', fullArgs)
}
