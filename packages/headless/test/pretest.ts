import rimrafModule = require('rimraf')
import fs = require('mz/fs')
import path = require('path')

const fixtures = path.join(__dirname, 'fixtures')
const workspaceFixture = path.join(__dirname, 'workspace-fixture')

removeNodeModules()
  .then(() => console.log('Done'))
  .catch(err => console.error(err))

async function removeNodeModules () {
  const dirsToRemove = [
    ...(await fs.readdir(fixtures)).map((dir) => path.join(fixtures, dir)),
    ...(await fs.readdir(workspaceFixture)).map((dir) => path.join(workspaceFixture, dir)),
    workspaceFixture,
  ]
  .map((dir) => path.join(dir, 'node_modules'))
  await Promise.all(dirsToRemove.map((dir) => rimraf(dir)))
}

function rimraf (dir: string) {
  return new Promise((resolve, reject) => rimrafModule(dir, err => err ? reject(err) : resolve()))
}
