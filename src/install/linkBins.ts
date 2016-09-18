import path = require('path')
import semver = require('semver')
import normalizePath = require('normalize-path')
import {stripIndent} from 'common-tags'
import relSymlink from '../fs/relSymlink'
import fs = require('mz/fs')
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import binify from '../binify'

const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')
const isWindows = process.platform === 'win32'

export default function linkAllBins (modules: string) {
  return Promise.all(
    getDirectories(modules)
      .reduce((pkgDirs: string[], dir: string): string[] => pkgDirs.concat(isScopedPkgsDir(dir) ? getDirectories(dir) : [dir]), [])
      .map(pkgDir => linkPkgBins(modules, pkgDir))
  )
}

function getDirectories (srcPath: string): string[] {
  let dirs: string[]
  try {
    dirs = fs.readdirSync(srcPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    dirs = []
  }
  return dirs
    .map(relativePath => path.join(srcPath, relativePath))
    .filter(absolutePath => fs.statSync(absolutePath).isDirectory())
}

function isScopedPkgsDir (dirPath: string) {
  return path.basename(dirPath)[0] === '@'
}

/**
 * Links executable into `node_modules/.bin`.
 *
 * @param {String} modules - the node_modules path
 * @param {String} target - where the module is now; read package.json from here
 *
 * @example
 *     module = 'project/node_modules'
 *     target = 'project/node_modules/.store/rimraf@2.5.1'
 *     linkPkgBins(module, target)
 *
 *     // node_modules/.bin/rimraf -> ../.store/rimraf@2.5.1/cmd.js
 */
export async function linkPkgBins (modules: string, target: string) {
  const pkg = tryRequire(path.join(target, 'package.json'))

  if (!pkg || !pkg.bin) return

  const bins = binify(pkg)
  const binDir = path.join(modules, '.bin')

  await mkdirp(binDir)
  await Promise.all(Object.keys(bins).map(async function (bin) {
    const actualBin = bins[bin]
    const externalBinPath = path.join(binDir, bin)

    const targetPath = normalizePath(path.join(pkg.name, actualBin))
    if (isWindows) {
      if (!preserveSymlinks) {
        return cmdShim(externalBinPath, '../' + targetPath)
      }
      const proxyFilePath = path.join(binDir, bin + '.proxy')
      fs.writeFileSync(proxyFilePath, 'require("../' + targetPath + '")', 'utf8')
      return cmdShim(externalBinPath, path.relative(binDir, proxyFilePath))
    }

    if (!preserveSymlinks) {
      await makeExecutable(path.join(target, actualBin))
      return relSymlink(
        path.join(target, actualBin),
        externalBinPath)
    }

    return proxy(externalBinPath, targetPath)
  }))
}

function makeExecutable (filePath: string) {
  return fs.chmod(filePath, 0o755)
}

function proxy (proxyPath: string, targetPath: string) {
  const proxyContent = stripIndent`
    #!/bin/sh
    ":" //# comment; exec /usr/bin/env node --preserve-symlinks "$0" "$@"
    require('../${targetPath}')`
  fs.writeFileSync(proxyPath, proxyContent, 'utf8')
  return makeExecutable(proxyPath)
}

function cmdShim (proxyPath: string, targetPath: string) {
  const nodeOptions = preserveSymlinks ? '--preserve-symlinks' : ''
  const cmdContent = stripIndent`
    @IF EXIST "%~dp0\\node.exe" (
      "%~dp0\\node.exe ${nodeOptions}"  "%~dp0/${targetPath}" %*
    ) ELSE (
      @SETLOCAL
      @SET PATHEXT=%PATHEXT:;.JS;=;%
      node ${nodeOptions}  "%~dp0/${targetPath}" %*
    )`
  fs.writeFileSync(proxyPath + '.cmd', cmdContent, 'utf8')

  const shContent = stripIndent`
    #!/bin/sh
    basedir=$(dirname "$(echo "$0" | sed -e 's,\\,/,g')")

    case \`uname\` in
        *CYGWIN*) basedir=\`cygpath -w "$basedir"\`;;
    esac

    if [ -x "$basedir/node" ]; then
      "$basedir/node ${nodeOptions}"  "$basedir/${targetPath}" "$@"
      ret=$?
    else
      node ${nodeOptions}  "$basedir/${targetPath}" "$@"
      ret=$?
    fi
    exit $ret
    `
  fs.writeFileSync(proxyPath, shContent, 'utf8')
  return Promise.all([
    makeExecutable(proxyPath + '.cmd'),
    makeExecutable(proxyPath)
  ])
}

/**
 * Like `require()`, but returns `undefined` when it fails
 */
function tryRequire (path: string) {
  try { return requireJson(path) } catch (e) { return null }
}
