'use strict'
const join = require('path').join
const basename = require('path').basename
const relative = require('path').relative
const semver = require('semver')
const normalizePath = require('normalize-path')
const relSymlink = require('../fs/rel_symlink')
const fs = require('mz/fs')
const mkdirp = require('../fs/mkdirp')
const requireJson = require('../fs/require_json')
const binify = require('../binify')

const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')
const isWindows = process.platform === 'win32'

module.exports = linkAllBins
module.exports.linkPkgBins = linkPkgBins

function linkAllBins (modules) {
  return Promise.all(
    getDirectories(modules)
      .reduce((pkgDirs, dir) => pkgDirs.concat(isScopedPkgsDir(dir) ? getDirectories(dir) : dir), [])
      .map(pkgDir => linkPkgBins(modules, pkgDir))
  )
}

function getDirectories (srcPath) {
  let dirs
  try {
    dirs = fs.readdirSync(srcPath)
  } catch (err) {
    if (err.code !== 'ENOENT') {
      throw err
    }
    dirs = []
  }
  return dirs
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
  const pkg = tryRequire(join(target, 'package.json'))

  if (!pkg || !pkg.bin) return

  const bins = binify(pkg)
  const binDir = join(modules, '.bin')

  return mkdirp(binDir)
    .then(() => Promise.all(Object.keys(bins).map(bin => {
      const actualBin = bins[bin]
      const externalBinPath = join(binDir, bin)

      const targetPath = normalizePath(join(pkg.name, actualBin))
      if (isWindows) {
        if (!preserveSymlinks) {
          return cmdShim(externalBinPath, '../' + targetPath)
        }
        const proxyFilePath = join(binDir, bin + '.proxy')
        fs.writeFileSync(proxyFilePath, 'require("../' + targetPath + '")', 'utf8')
        return cmdShim(externalBinPath, relative(binDir, proxyFilePath))
      }

      if (!preserveSymlinks) {
        return makeExecutable(join(target, actualBin))
          .then(_ => relSymlink(
            join(target, actualBin),
            externalBinPath))
      }

      return proxy(externalBinPath, targetPath)
    })))
}

function makeExecutable (filePath) {
  return fs.chmod(filePath, 0o755)
}

function proxy (proxyPath, targetPath) {
  const proxyContent = [
    '#!/bin/sh',
    '":" //# comment; exec /usr/bin/env node --preserve-symlinks "$0" "$@"',
    "require('../" + targetPath + "')"
  ].join('\n')
  fs.writeFileSync(proxyPath, proxyContent, 'utf8')
  return makeExecutable(proxyPath)
}

function cmdShim (proxyPath, targetPath) {
  const nodeOptions = preserveSymlinks ? '--preserve-symlinks' : ''
  const cmdContent = [
    '@IF EXIST "%~dp0\\node.exe" (',
    '  "%~dp0\\node.exe ' + nodeOptions + '"  "%~dp0/' + targetPath + '" %*',
    ') ELSE (',
    '  @SETLOCAL',
    '  @SET PATHEXT=%PATHEXT:;.JS;=;%',
    '  node ' + nodeOptions + '  "%~dp0/' + targetPath + '" %*',
    ')'
  ].join('\n')
  fs.writeFileSync(proxyPath + '.cmd', cmdContent, 'utf8')

  const shContent = [
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
