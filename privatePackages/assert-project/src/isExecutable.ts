import { promisify } from 'util'
import isWindows = require('is-windows')
import isexeCB = require('isexe')
import fs = require('mz/fs')

const IS_WINDOWS = isWindows()
const isexe = promisify(isexeCB)

// eslint-disable-next-line
export default async (ok: (value: any, comment: string) => void, filePath: string) => {
  if (IS_WINDOWS) {
    ok(await isexe(`${filePath}.cmd`), `${filePath}.cmd is executable`)
    return
  }

  const stat = await fs.stat(filePath)
  ok((stat.mode & 0o111) === 0o111, `${filePath} is executable`)
  ok(stat.isFile(), `${filePath} refers to a file`)
}
