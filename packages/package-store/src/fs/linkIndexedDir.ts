import pnpmLogger, { storeLogger } from '@pnpm/logger'
import makeDir = require('make-dir')
import fs = require('mz/fs')
import path = require('path')
import pathTemp = require('path-temp')
import rimraf = require('rimraf-then')

const hardLinkingLogger = pnpmLogger('_hardlink-already-exists')

export default async function linkIndexedDir (existingDir: string, newDir: string, filenames: string[]) {
  const stage = pathTemp(path.dirname(newDir))
  try {
    await rimraf(stage)
    await tryLinkIndexedDir(existingDir, stage, filenames)
    await rimraf(newDir)
    await fs.rename(stage, newDir)
  } catch (err) {
    try { await rimraf(stage) } catch (err) {} // tslint:disable-line:no-empty
    throw err
  }
}

async function tryLinkIndexedDir (existingDir: string, newDir: string, filenames: string[]) {
  const alldirs = new Set()
  filenames
    .forEach((f) => {
      alldirs.add(path.join(newDir, path.dirname(f)))
    })
  await Promise.all(
    Array.from(alldirs).sort((d1, d2) => d1.length - d2.length).map((dir) => makeDir(dir)),
  )
  let allLinked = true
  await Promise.all(
    filenames
      .map(async (f: string) => {
        const src = path.join(existingDir, f)
        const dest = path.join(newDir, f)
        try {
          await fs.link(src, dest)
        } catch (err) {
          if (err['code'] !== 'EEXIST') throw err
          // If the file is already linked, we ignore the error.
          // This is an extreme edge case that may happen only in one case,
          // when the store folder is case sensitive and the project's node_modules
          // is case insensitive.
          // So, for instance, foo.js and Foo.js could be unpacked to the store
          // but they cannot be both linked to node_modules.
          // More details at https://github.com/pnpm/pnpm/issues/1685
          allLinked = false
          hardLinkingLogger.debug({ src, dest })
        }
      }),
  )
  if (!allLinked) {
    storeLogger.warn(
      `Not all files from "${existingDir}" were linked to "${newDir}". ` +
      'This happens when the store is case sensitive while the target directory is case insensitive.',
    )
  }
}
