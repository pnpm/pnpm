import { fs as memfs } from 'memfs'
import path from 'node:path'

const { fixtures, fixtures2 } = process.platform === 'win32' ? {
  fixtures: 'I:\\cmd-shim\\fixtures',
  fixtures2: 'J:\\cmd-shim\\fixtures'
} : {
  fixtures: '/foo/cmd-shim/fixtures',
  fixtures2: '/bar/cmd-shim/fixtures'
}

export { fixtures, fixtures2, memfs as fs }

/** @type {{ [dir: string]: { [filename: string]: string } }} */
const fixtureFiles = {
  [fixtures]: {
    'src.exe': 'exe',
    'src.bat': 'bat',
    'src.env': '#!/usr/bin/env node\nconsole.log(/hi/)\n',
    'src.env.args': '#!/usr/bin/env node --expose_gc\ngc()\n',
    'src.sh': '#!/usr/bin/sh\necho hi\n',
    'src.sh.args': '#!/usr/bin/sh -x\necho hi\n',
    'from.env.S': '#!/usr/bin/env -S node --expose_gc\ngc()\n'
  },
  [fixtures2]: {
    'src.sh.args': '#!/usr/bin/sh -x\necho hi\n'
  }
}

export async function setupFixtures () {
  const fsPromises = memfs.promises
  await Promise.all(
    Object.entries(fixtureFiles).map(async ([dir, files]) => {
      await fsPromises.mkdir(dir, { recursive: true })
      return Promise.all(
        Object.entries(files).map(([filename, contents]) => {
          return fsPromises.writeFile(path.join(dir, filename), contents)
        })
      )
    })
  )
}
