import pkgFullName, {delimiter} from '../pkgFullName'
import {HostedPackageToResolve, ResolveOptions} from '../resolve'
import {Package} from '../api/initCmd'

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */

const PARSE_GITHUB_RE = /^github:([^\/]+)\/([^#]+)(#(.+))?$/

export default async function resolveGithub (pkg: HostedPackageToResolve, opts: ResolveOptions) {
  const getJSON = opts.got.getJSON
  const spec = parseGithubSpec(pkg)
  spec.ref = await resolveRef(spec)
  const resPkg: Package = await resolvePackageJson(spec)
  return {
    name: resPkg.name,
    version: resPkg.version,
    fullname: pkgFullName({
      name: resPkg.name,
      version: ['github', spec.owner, spec.repo, spec.ref].join(delimiter)
    }),
    dist: {
      tarball: [
        'https://api.github.com/repos',
        spec.owner,
        spec.repo,
        'tarball',
        spec.ref
      ].join('/')
    }
  }

  type GitHubContentResponse = {
    content: string
  }

  async function resolvePackageJson (spec: GitHubSpec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'contents/package.json?ref=' + spec.ref
    ].join('/')
    const body = await getJSON<GitHubContentResponse>(url)
    const content = new Buffer(body.content, 'base64').toString('utf8')
    return JSON.parse(content)
  }

  type GitHubRepoResponse = {
    sha: string
  }

  async function resolveRef (spec: GitHubSpec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'commits',
      spec.ref
    ].join('/')
    const body = await getJSON<GitHubRepoResponse>(url)
    return body.sha
  }
}

function parseGithubSpec (pkg: HostedPackageToResolve): GitHubSpec {
  const m = PARSE_GITHUB_RE.exec(pkg.hosted.shortcut)
  if (!m) {
    throw new Error('cannot parse: ' + pkg.hosted.shortcut)
  }
  const owner = m[1]
  const repo = m[2]
  const ref = m[4] || 'HEAD'
  return {owner, repo, ref}
}

type GitHubSpec = {
  owner: string,
  repo: string,
  ref: string
}
