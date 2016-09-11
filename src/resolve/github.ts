import pkgFullName, {delimiter} from '../pkg_full_name'
import {PackageToResolve, ResolveOptions} from '../resolve'
import {Package} from '../api/init_cmd'

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */

const PARSE_GITHUB_RE = /^github:([^\/]+)\/([^#]+)(#(.+))?$/

export default function resolveGithub (pkg: PackageToResolve, opts: ResolveOptions) {
  const getJSON = opts.got.getJSON
  const spec = parseGithubSpec(pkg)
  return resolveRef(spec).then((ref: string) => {
    spec.ref = ref
    return resolvePackageJson(spec).then((pkg: Package) => ({
      name: pkg.name,
      version: pkg.version,
      fullname: pkgFullName({
        name: pkg.name,
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
    }))
  })

  type GitHubContentResponse = {
    content: string
  }

  function resolvePackageJson (spec: GitHubSpec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'contents/package.json?ref=' + spec.ref
    ].join('/')
    return getJSON(url).then((body: GitHubContentResponse) => {
      const content = new Buffer(body.content, 'base64').toString('utf8')
      return JSON.parse(content)
    })
  }

  type GitHubRepoResponse = {
    sha: string
  }

  function resolveRef (spec: GitHubSpec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'commits',
      spec.ref
    ].join('/')
    return getJSON(url).then((body: GitHubRepoResponse) => body.sha)
  }
}

function parseGithubSpec (pkg: PackageToResolve): GitHubSpec {
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
