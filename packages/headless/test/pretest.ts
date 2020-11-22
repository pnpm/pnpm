import path = require('path')
import fs = require('mz/fs')
import rimrafModule = require('rimraf')

const fixtures = path.join(__dirname, 'fixtures')
const workspaceFixture = path.join(__dirname, 'workspace-fixture')
const workspaceFixture2 = path.join(__dirname, 'workspace-fixture2')

removeModules()
  .then(() => console.log('Done'))
  .catch(err => console.error(err))

async function removeModules () {
  const dirsToRemove = [
    ...(await fs.readdir(fixtures)).map((dir) => path.join(fixtures, dir)),
    ...(await fs.readdir(workspaceFixture)).map((dir) => path.join(workspaceFixture, dir)),
    ...(await fs.readdir(workspaceFixture2)).map((dir) => path.join(workspaceFixture2, dir)),
    workspaceFixture,
    workspaceFixture2,
  ]
    .map((dir) => path.join(dir, 'node_modules'))
  await Promise.all(dirsToRemove.map((dir) => rimraf(dir)))
}

function rimraf (dir: string) {
  return new Promise<void>((resolve, reject) => rimrafModule(dir, err => err ? reject(err) : resolve()))
}
