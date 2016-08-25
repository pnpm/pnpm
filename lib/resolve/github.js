'use strict'
const pkgFullName = require('../pkg_full_name')

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */

const PARSE_GITHUB_RE = /^github:([^\/]+)\/([^#]+)(#(.+))?$/

module.exports = function resolveGithub (pkg, opts) {
  const getJSON = opts.got.getJSON
  const spec = parseGithubSpec(pkg)
  return resolveRef(spec).then(ref => {
    spec.ref = ref
    return resolvePackageName(spec).then(name => {
      return {
        name,
        fullname: pkgFullName({
          name,
          version: ['github', spec.owner, spec.repo, spec.ref].join(pkgFullName.delimiter)
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
    })
  })

  function resolvePackageName (spec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'contents/package.json?ref=' + spec.ref
    ].join('/')
    return getJSON(url).then(body => {
      const content = new Buffer(body.content, 'base64').toString('utf8')
      const pkg = JSON.parse(content)
      return pkg.name
    })
  }

  function resolveRef (spec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'commits',
      spec.ref
    ].join('/')
    return getJSON(url).then(body => body.sha)
  }
}

function parseGithubSpec (pkg) {
  const m = PARSE_GITHUB_RE.exec(pkg.hosted.shortcut)
  if (!m) {
    throw new Error('cannot parse: ' + pkg.hosted.shortcut)
  }
  const owner = m[1]
  const repo = m[2]
  const ref = m[4] || 'HEAD'
  return {owner, repo, ref}
}
