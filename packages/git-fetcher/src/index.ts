import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import tempy = require('tempy')

export default () => {
  return {
    git: async function fetchFromGit (
      cafs: Cafs,
      resolution: {
        commit: string
        repo: string
        type: 'git'
      },
      opts: {
        manifest?: DeferredManifestPromise
      }
    ) {
      const tempLocation = tempy.directory()
      await execGit(['clone', resolution.repo, tempLocation])
      await execGit(['checkout', resolution.commit], { cwd: tempLocation })
      // removing /.git to make directory integrity calculation faster
      await rimraf(path.join(tempLocation, '.git'))
      return {
        filesIndex: await cafs.addFilesFromDir(tempLocation, opts.manifest),
      }
    },
  }
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function execGit (args: string[], opts?: object) {
  const fullArgs = prefixGitArgs().concat(args || [])
  return execa('git', fullArgs, opts)
}
