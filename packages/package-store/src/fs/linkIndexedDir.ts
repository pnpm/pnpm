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
  await Promise.all(
    filenames
      .map((f: string) => fs.link(path.join(existingDir, f), path.join(newDir, f))),
  )
}
