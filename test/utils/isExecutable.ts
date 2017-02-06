import isexe = require('isexe')
import fs = require('mz/fs')
import {Test} from 'tape'
import semver = require('semver')
import {Stats} from 'fs'

export default async function isExecutable (t: Test, filePath: string) {
  t.ok(isexe(filePath), filePath + ' is executable')
}
