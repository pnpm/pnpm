import spawn = require('cross-spawn')
import {PackageSpec, ResolveOptions, ResolveResult} from '.'
import {delimiter} from './createPkgId'
import createDebug from '../debug'
const debug = createDebug('pnpm:git')

export default async function resolveGithub (parsedSpec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const parts = parsedSpec.spec.split('#')
  const repo = parts[0]
  const ref = parts[1] || 'master'
  return {
    id: parsedSpec.spec.replace(/\//g, delimiter),
    fetch: (target: string) => {
      return clone(repo, ref, target)
    }
  }
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function spawnGit (args: string[]) {
  debug(`executing git with args ${args}`)
  const fullArgs = prefixGitArgs().concat(args || [])
  return spawn('git', fullArgs)
}

/**
 * clone a git repository.
 */
export function clone (repo: string, ref: string, dest: string) {
  return new Promise((resolve, reject) => {
    const args = ['clone', '-b', ref, repo, dest, '--single-branch']
    debug(`cloning git repository from ${repo}`)
    const git = spawnGit(args)
    let errMsg = ''
    git.stderr.on('data', (data: string) => errMsg += data)
    git.on('close', (code: number) => (code ? errorHandler() : resolve()))

    function errorHandler () {
      debug(`failed to clone repository from ${repo}`)
      reject(new Error(`failed to clone repository from ${repo}
        ${errMsg}`))
    }
  })
}
