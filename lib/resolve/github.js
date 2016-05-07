var got = require('../got')

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */

var PARSE_GITHUB_RE = /^github:([^\/]+)\/([^#]+)(#(.+))?$/

module.exports = function resolveGithub (pkg) {
  var spec = parseGithubSpec(pkg)
  return resolveRef(spec).then(function (ref) {
    spec.ref = ref
    return resolvePackageName(spec).then(function (name) {
      var fullname = name + ['@github', spec.owner, spec.repo, spec.ref].join('!')
      return {
        name: name,
        fullname: fullname,
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
}

function resolvePackageName (spec) {
  var url = [
    'https://api.github.com/repos',
    spec.owner,
    spec.repo,
    'contents/package.json?ref=' + spec.ref
  ].join('/')
  return getJSON(url).then(function (body) {
    var content = new Buffer(body.content, 'base64').toString('utf8')
    var pkg = JSON.parse(content)
    return pkg.name
  })
}

function resolveRef (spec) {
  var url = [
    'https://api.github.com/repos',
    spec.owner,
    spec.repo,
    'commits',
    spec.ref
  ].join('/')
  return getJSON(url).then(function (body) { return body.sha })
}

function getJSON (url) {
  return got.get(url)
    .then(function (res) { return res.promise })
    .then(function (res) {
      var body = JSON.parse(res.body)
      return body
    })
}

function parseGithubSpec (pkg) {
  var m = PARSE_GITHUB_RE.exec(pkg.hosted.shortcut)
  if (!m) {
    throw new Error('cannot parse: ' + pkg.hosted.shortcut)
  }
  var owner = m[1]
  var repo = m[2]
  var ref = m[4] || 'HEAD'
  return {owner: owner, repo: repo, ref: ref}
}
