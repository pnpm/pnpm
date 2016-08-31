'use strict'
const fs = require('fs')
const path = require('path')

const cwd = process.cwd()

const oldDirName = process.argv[2]
const newDirName = process.argv[3]

renameDirectories(cwd)

function renameDirectories (srcpath) {
  return fs.readdirSync(srcpath)
    .map(file => path.join(srcpath, file))
    .filter(file => fs.statSync(file).isDirectory())
    .forEach(file => {
      renameDirectories(file)
      const parts = file.split(path.sep)
      if (parts[parts.length - 1] === oldDirName) {
        const newDirPath = (parts.slice(0, -1).concat([newDirName])).join(path.sep)
        fs.renameSync(file, newDirPath)
      }
    })
}
