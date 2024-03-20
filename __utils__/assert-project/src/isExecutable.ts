import { promises as fs } from 'node:fs'
import { promisify } from 'node:util'

import isWindows from 'is-windows'
import isexeCB from 'isexe'

const IS_WINDOWS = isWindows()
const isexe = promisify(isexeCB.isexe)

export async function isExecutable(
  ok: (value: unknown, comment: string) => void,
  filePath: string
): Promise<void> {
  if (IS_WINDOWS) {
    ok(
      await isexe(`${filePath}.cmd`, undefined),
      `${filePath}.cmd is executable`
    )
    return
  }

  const stat = await fs.stat(filePath)

  ok((stat.mode & 0o1_1_1) === 0o1_1_1, `${filePath} is executable`)

  ok(stat.isFile(), `${filePath} refers to a file`)
}
