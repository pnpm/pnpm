var join = require('path').join
var basename = require('path').basename
var relative = require('path').relative
var semver = require('semver')
var normalizePath = require('normalize-path')
var relSymlink = require('../fs/rel_symlink')
var fs = require('mz/fs')
var mkdirp = require('../fs/mkdirp')
var debug = require('debug')('pnpm:link_bins')
var requireJson = require('../fs/require_json')
var binify = require('../binify')

var preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')
var isWindows = process.platform === 'win32'

module.exports = linkAllBins
module.exports.linkPkgBins = linkPkgBins

function linkAllBins (modules) {
  getDirectories(modules)
    .reduce((pkgDirs, dir) => pkgDirs.concat(isScopedPkgsDir(dir) ? getDirectories(dir) : dir), [])
    .forEach(pkgDir => linkPkgBins(modules, pkgDir))
}

function getDirectories (srcPath) {
  return fs.readdirSync(srcPath)
    .map(relativePath => join(srcPath, relativePath))
    .filter(absolutePath => fs.statSync(absolutePath).isDirectory())
}

function isScopedPkgsDir (dirPath) {
  return basename(dirPath)[0] === '@'
}

/*
 * Links executable into `node_modules/.bin`.
 *
 * - `modules` (String) - the node_modules path
 * - `target` (String) - where the module is now; read package.json from here
 * - `fullname` (String) - fullname of the module (`rimraf@2.5.1`)
 *
 *     module = 'project/node_modules'
 *     target = 'project/node_modules/.store/rimraf@2.5.1'
 *     linkPkgBins(module, target)
 *
 *     // node_modules/.bin/rimraf -> ../.store/rimraf@2.5.1/cmd.js
 */

function linkPkgBins (modules, target) {
  var pkg = tryRequire(join(target, 'package.json'))

  if (!pkg || !pkg.bin) return

  var bins = binify(pkg)

  return Promise.all(Object.keys(bins).map(bin => {
    var actualBin = bins[bin]
    var binDir = join(modules, '.bin')
    var externalBinPath = join(binDir, bin)

    return Promise.resolve()
      .then(_ => mkdirp(join(modules, '.bin')))
      .then(_ => {
        var targetPath = normalizePath(join(pkg.name, actualBin))
        if (isWindows) {
          if (!preserveSymlinks) {
            return cmdShim(externalBinPath, '../' + targetPath)
          }
          var proxyFilePath = join(binDir, bin + '.proxy')
          fs.writeFileSync(proxyFilePath, 'require("../' + targetPath + '")', 'utf8')
          return cmdShim(externalBinPath, relative(binDir, proxyFilePath))
        }

        if (!preserveSymlinks) {
          return makeExecutable(join(target, actualBin))
            .then(_ => debug('linking %s -> %s',
              join(target, actualBin),
              externalBinPath))
            .then(_ => relSymlink(
              join(target, actualBin),
              externalBinPath))
        }

        return proxy(externalBinPath, targetPath)
      })
  }))
}

function makeExecutable (filePath) {
  return fs.chmod(filePath, 0o755)
}

function proxy (proxyPath, targetPath) {
  var proxyContent = [
    '#!/bin/sh',
    '":" //# comment; exec /usr/bin/env node --preserve-symlinks "$0" "$@"',
    "require('../" + targetPath + "')"
  ].join('\n')
  fs.writeFileSync(proxyPath, proxyContent, 'utf8')
  return makeExecutable(proxyPath)
}

function cmdShim (proxyPath, targetPath) {
  var nodeOptions = preserveSymlinks ? '--preserve-symlinks' : ''
  var cmdContent = [
    '@IF EXIST "%~dp0\\node.exe" (',
    '  "%~dp0\\node.exe ' + nodeOptions + '"  "%~dp0/' + targetPath + '" %*',
    ') ELSE (',
    '  @SETLOCAL',
    '  @SET PATHEXT=%PATHEXT:;.JS;=;%',
    '  node ' + nodeOptions + '  "%~dp0/' + targetPath + '" %*',
    ')'
  ].join('\n')
  fs.writeFileSync(proxyPath + '.cmd', cmdContent, 'utf8')

  var shContent = [
    '#!/bin/sh',
    'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\,/,g\')")',
    '',
    'case `uname` in',
    '    *CYGWIN*) basedir=`cygpath -w "$basedir"`;;',
    'esac',
    '',
    'if [ -x "$basedir/node" ]; then',
    '  "$basedir/node ' + nodeOptions + '"  "$basedir/' + targetPath + '" "$@"',
    '  ret=$?',
    'else ',
    '  node ' + nodeOptions + '  "$basedir/' + targetPath + '" "$@"',
    '  ret=$?',
    'fi',
    'exit $ret',
    ''
  ].join('\n')
  fs.writeFileSync(proxyPath, shContent, 'utf8')
  return Promise.all([
    makeExecutable(proxyPath + '.cmd'),
    makeExecutable(proxyPath)
  ])
}

/*
 * Like `require()`, but returns `undefined` when it fails
 */

function tryRequire (path) {
  try { return requireJson(path) } catch (e) { }
}
