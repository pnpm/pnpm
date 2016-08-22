'use strict'
const fs = require('fs')
const path = require('path')

module.exports = opts => {
  const pkgPath = path.resolve(process.cwd(), 'package.json')
  const rawPkgJSON = fs.readFileSync(pkgPath, 'UTF8')
  const pkgJSON = JSON.parse(rawPkgJSON)

  const newPkgJSON = {}
  const keys = Object.keys(pkgJSON)
  for (let i = 0; i < keys.length; i++) {
    if (keys[i] === 'scripts') {
      newPkgJSON.scripts = pkgJSON.scripts
      if (opts.pkgName === 'pnpm-rocket') {
        newPkgJSON.scripts.preinstall = 'node .scripts/rename cached_node_modules node_modules'
        continue
      }
      delete newPkgJSON.scripts.preinstall
      continue
    }
    if (keys[i] === opts.currentKeyName) {
      newPkgJSON[opts.newKeyName] = pkgJSON[opts.currentKeyName]
      continue
    }
    newPkgJSON[keys[i]] = pkgJSON[keys[i]]
  }
  newPkgJSON.name = opts.pkgName

  fs.writeFileSync(pkgPath, JSON.stringify(newPkgJSON, null, 2), 'UTF8')
}
