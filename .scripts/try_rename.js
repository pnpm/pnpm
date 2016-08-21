'use strict'
const fs = require('fs')
const path = require('path')

const cwd = process.cwd()

const oldPath = path.resolve(cwd, process.argv[2])
const newPath = path.resolve(cwd, process.argv[3])

// should not fail because when npm install is executed locally, there is no cached_node_modules
try {
  fs.renameSync(oldPath, newPath)
} catch (err) {

}
