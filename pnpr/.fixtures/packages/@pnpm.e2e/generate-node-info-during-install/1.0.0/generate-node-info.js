const fs = require('fs')
const file = require('./file')
const nodeInfo = {
  execPath: process.execPath,
  versions: process.versions,
}
const json = JSON.stringify(nodeInfo, undefined, 2) + '\n'
console.log(json)
fs.writeFileSync(file.FULL_PATH, json)
