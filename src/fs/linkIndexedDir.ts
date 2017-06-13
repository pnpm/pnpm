import path = require('path')
import fs = require('mz/fs')
import pLimit = require('p-limit')
import mkdirp = require('mkdirp-promise')
import rimraf = require('rimraf-then')

const limitHardLinking = pLimit(20)

export default async function linkIndexedDir (existingDir: string, newDir: string, index: {}) {
  const stage = `${newDir}+stage`
  try {
    await rimraf(stage)
    await tryLinkIndexedDir(existingDir, stage, index)
    await rimraf(newDir)
    await fs.rename(stage, newDir)
  } catch (err) {
    try { await rimraf(stage) } catch (err) {}
    throw err
  }
}

async function tryLinkIndexedDir (existingDir: string, newDir: string, index: {}) {
  const alldirs = new Set()
  Object.keys(index)
    .forEach(f => {
      alldirs.add(path.join(newDir, path.dirname(f)))
    })
  await Promise.all(
    Array.from(alldirs).sort((d1, d2) => d1.length - d2.length).map(dir => mkdirp(dir))
  )
  await Promise.all(
    Object.keys(index)
      .filter(f => !index[f].isDir)
      .map((f: string) =>
        limitHardLinking(async () => {
          await fs.link(path.join(existingDir, f), path.join(newDir, f))
        }))
  )
}
