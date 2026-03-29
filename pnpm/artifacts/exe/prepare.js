import fs from 'fs'
import path from 'path'

const ownDir = import.meta.dirname
const placeholder = 'This file intentionally left blank'

// pnpm and pn are placeholders — replaced with hardlinks by setup.js
for (const name of ['pnpm', 'pn']) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, placeholder, 'utf8')
}

// pnpx and pnx — write the real shell scripts and Windows wrappers
for (const [name, command] of [['pnpx', 'pnpm dlx'], ['pnx', 'pnpm dlx']]) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, `#!/bin/sh\nexec ${command} "$@"\n`, { mode: 0o755 })
  fs.writeFileSync(path.join(ownDir, name + '.cmd'), `@echo off\n${command} %*\n`)
  fs.writeFileSync(path.join(ownDir, name + '.ps1'), `${command} @args\n`)
}
