'use strict'
const fs = require('fs')
const path = require('path')

const name = 'own-bin-created-by-preinstall'
const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1'] : ['']
const shims = process.env.PATH.split(path.delimiter)
  .flatMap((dir) => extensions.map((ext) => path.join(dir, name + ext)))
  .filter((file) => fs.existsSync(file))
if (shims.length > 0) {
  console.error(`${name} is on PATH before its target exists: ${shims.join(', ')}`)
  process.exit(1)
}

fs.mkdirSync(path.join(__dirname, 'bin'))
fs.writeFileSync(path.join(__dirname, 'bin', 'cli.js'), "console.log('created by preinstall')\n")
