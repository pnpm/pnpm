import { promisify } from 'util'
import { Test } from 'tape'
import isWindows = require('is-windows')
import isexeCB = require('isexe')
import fs = require('mz/fs')

const IS_WINDOWS = isWindows()
const isexe = promisify(isexeCB)

export default async (t: Test, filePath: string) => {
  if (IS_WINDOWS) {
    t.ok(await isexe(`${filePath}.cmd`), `${filePath}.cmd is executable`)
    return
  }

  const stat = await fs.stat(filePath)
  t.ok((stat.mode & 0o111) === 0o111, `${filePath} is executable`)
  t.ok(stat.isFile(), `${filePath} refers to a file`)
}
