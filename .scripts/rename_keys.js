'use strict'
const fs = require('fs')
const path = require('path')
const eof = require('os').EOL

module.exports = keysMap => {
  const pkgPath = path.resolve(process.cwd(), 'package.json')
  const rawPkgJSON = fs.readFileSync(pkgPath, 'UTF8')
  const pkgJSON = JSON.parse(rawPkgJSON)

  const newPkgJSON = {}
  const keys = Object.keys(pkgJSON)
  for (let i = 0; i < keys.length; i++) {
    if (keysMap[keys[i]]) {
      newPkgJSON[keysMap[keys[i]]] = pkgJSON[keys[i]]
      continue
    }
    newPkgJSON[keys[i]] = pkgJSON[keys[i]]
  }

  fs.writeFileSync(pkgPath, JSON.stringify(newPkgJSON, null, 2) + eof, 'UTF8')
}
