import rimraf = require('rimraf')
import fs = require('mz/fs')
import path = require('path')

const fixtures = path.join(__dirname, 'fixtures')

removeNodeModules()
  .then(() => console.log('Done'))
  .catch(err => console.error(err))

async function removeNodeModules () {
  await Promise.all(
    (await fs.readdir(fixtures))
      .map((dir) =>
        new Promise((resolve, reject) =>
          rimraf(path.join(fixtures, dir, 'node_modules'), err => err ? reject(err) : resolve())))
  )
}
