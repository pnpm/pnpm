import pkgFullName, {delimiter} from '../pkgFullName'
import {HostedPackageSpec, ResolveOptions, ResolveResult} from '.'
import {Package} from '../api/initCmd'

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */
const PARSE_GITHUB_RE = /^github:([^\/]+)\/([^#]+)(#(.+))?$/

export default async function resolveGithub (spec: HostedPackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const getJSON = opts.got.getJSON
  const ghSpec = parseGithubSpec(spec)
  ghSpec.ref = await resolveRef(ghSpec)
  const resPkg: Package = await resolvePackageJson(ghSpec)
  return {
    fullname: pkgFullName({
      name: resPkg.name,
      version: ['github', ghSpec.owner, ghSpec.repo, ghSpec.ref].join(delimiter)
    }),
    dist: {
      location: 'remote',
      tarball: [
        'https://api.github.com/repos',
        ghSpec.owner,
        ghSpec.repo,
        'tarball',
        ghSpec.ref
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

function parseGithubSpec (pkg: HostedPackageSpec): GitHubSpec {
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
