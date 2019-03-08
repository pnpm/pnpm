import dint = require('dint')
import execa = require('execa')
import path = require('path')
import pathTemp = require('path-temp')
import rimraf = require('rimraf-then')

export default () => {
  return {
    git: async function fetchFromGit (
      resolution: {
        repo: string,
        commit: string,
      },
      targetFolder: string,
    ) {
      const tempLocation = pathTemp(targetFolder)
      await execGit(['clone', resolution.repo, tempLocation])
      await execGit(['checkout', resolution.commit], { cwd: tempLocation })
      // removing /.git to make directory integrity calculation faster
      await rimraf(path.join(tempLocation, '.git'))
      return {
        filesIndex: await dint.from(tempLocation),
        tempLocation,
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
