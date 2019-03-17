import { storeLogger } from '@pnpm/logger'
import mkdirp = require('mkdirp-promise')
import fs = require('mz/fs')
import path = require('path')
import pathTemp = require('path-temp')
import rimraf = require('rimraf-then')

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
    Array.from(alldirs).sort((d1, d2) => d1.length - d2.length).map((dir) => mkdirp(dir)),
  )
  let allLinked = true
  await Promise.all(
    filenames
      .map(async (f: string) => {
        try {
          await fs.link(path.join(existingDir, f), path.join(newDir, f))
        } catch (err) {
          if (err['code'] !== 'EEXIST') throw err
          // If the file is already linked, we ignore the error.
          // This is an extreme edge case that may happen only in one case,
          // when the store folder is case sensitive and the project's node_modules
          // is case insensitive.
          // So, for instance, foo.js and FOO.js could be unpacked to the store
          // but they cannot be both linked to node_modules.
          // More details at https://github.com/pnpm/pnpm/issues/1685
          allLinked = false
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
