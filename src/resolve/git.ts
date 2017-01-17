import execa = require('execa')
import {PackageSpec, ResolveOptions, ResolveResult} from '.'
import {delimiter} from './createPkgId'
import hostedGitInfo = require('@zkochan/hosted-git-info')

// TODO: resolve ref to commit
export default async function resolveGit (parsedSpec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const parts = normalizeRepoUrl(parsedSpec.spec).split('#')
  const repo = parts[0]
  const ref = parts[1] || 'master'
  const commitId = await resolveRef(repo, ref)
  return {
    id: repo
      .replace(/^.*:\/\/(git@)?/, '')
      .replace(/:/g, delimiter)
      .replace(/\.git$/, '') + '/' + commitId,
    repo,
    commitId,
  }
}

async function resolveRef(repo: string, ref: string) {
  const result = await execa('git', ['ls-remote', '--refs', repo, ref])
  // should output something like:
  //   572bc3d4e16220c2e986091249e62a5913294b25    	refs/heads/master

  // if no ref was found, assume that ref is the commit ID
  if (!result.stdout) return ref

  return result.stdout.match(/^[a-z0-9]+/)
}

function normalizeRepoUrl (repoUrl: string) {
  const hosted = hostedGitInfo.fromUrl(repoUrl)
  if (!hosted) return repoUrl
  return hosted.getDefaultRepresentation() == 'shortcut' ? hosted.git() : hosted.toString()
}
