import isexe = require('isexe')
import fs = require('fs')
import {Test} from 'tape'
import semver = require('semver')

const isWindows = process.platform === 'win32'
const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')

export default function isExecutable (t: Test, filePath: string) {
  if (!isWindows && !preserveSymlinks) {
    const lstat = fs.lstatSync(filePath)
    t.ok(lstat.isSymbolicLink(), filePath + ' symlink is available')

    const stat = fs.statSync(filePath)
    t.equal(stat.mode, parseInt('100755', 8), filePath + ' is executable')
    t.ok(stat.isFile(), filePath + ' refers to a file')
    return
  }
  t.ok(isexe(filePath), filePath + ' is executable')
}
