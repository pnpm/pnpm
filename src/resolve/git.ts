import spawn = require('cross-spawn')
import {PackageSpec, ResolveOptions, ResolveResult} from '.'
import {delimiter} from './createPkgId'

// TODO: resolve ref to commit
export default async function resolveGithub (parsedSpec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const parts = parsedSpec.spec.split('#')
  const repo = parts[0]
  const ref = parts[1] || 'master'
  return {
    id: parsedSpec.spec
      .replace(/^.*:\/\/(git@)?/, '')
      .replace(/:/g, delimiter)
      .replace(/\.git$/, ''),
    repo,
    ref,
  }
}
