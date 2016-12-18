import isexe = require('isexe')
import fs = require('mz/fs')
import {Test} from 'tape'
import semver = require('semver')
import {Stats} from 'fs'

const isWindows = process.platform === 'win32'
const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')

export default async function isExecutable (t: Test, filePath: string) {
  if (!isWindows && !preserveSymlinks) {
    const lstat: Stats = await fs.lstat(filePath)
    t.ok(lstat.isSymbolicLink(), filePath + ' symlink is available')

    const stat = await fs.stat(filePath)
    t.equal(stat.mode, parseInt('100755', 8), filePath + ' is executable')
    t.ok(stat.isFile(), filePath + ' refers to a file')
    return
  }
  t.ok(isexe(filePath), filePath + ' is executable')
}
