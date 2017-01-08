import isexe = require('isexe')
import fs = require('mz/fs')
import {Test} from 'tape'
import semver = require('semver')
import {Stats} from 'fs'

const isWindows = process.platform === 'win32'

export default async function isExecutable (t: Test, filePath: string) {
  t.ok(isexe(filePath), filePath + ' is executable')
}
