import path from 'path'
import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import preparePackage from '@pnpm/prepare-package'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'

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
      const tempLocation = await cafs.tempDir()
      await execGit(['clone', resolution.repo, tempLocation])
      await execGit(['checkout', resolution.commit], { cwd: tempLocation })
      await preparePackage(tempLocation)
      // removing /.git to make directory integrity calculation faster
      await rimraf(path.join(tempLocation, '.git'))
      const filesIndex = await cafs.addFilesFromDir(tempLocation, opts.manifest)
      await rimraf(tempLocation)
      return { filesIndex }
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
