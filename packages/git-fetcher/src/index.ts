import path from 'path'
import { Cafs, DeferredManifestPromise } from '@pnpm/fetcher-base'
import { runPrepareHook, filterFilesIndex } from '@pnpm/prepare-package'
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
      // removing /.git to make directory integrity calculation faster
      await rimraf(path.join(tempLocation, '.git'))
      await runPrepareHook(tempLocation)
      const filesIndex = await filterFilesIndex(tempLocation, await cafs.addFilesFromDir(tempLocation, opts.manifest))
      // Important! We cannot remove the temp location at this stage.
      // Even though we have the index of the package,
      // the linking of files to the store is in progress.
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
