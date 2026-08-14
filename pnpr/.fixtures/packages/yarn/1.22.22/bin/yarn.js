#!/usr/bin/env node
'use strict'

// Stand-in for Yarn Classic in tests that provision a package manager.
// Prints its version, and records every other invocation in the working
// directory so a test can assert which package manager ran there.
const fs = require('fs')
const path = require('path')

const args = process.argv.slice(2)
if (args[0] === '--version' || args[0] === '-v') {
  console.log(require('../package.json').version)
} else {
  fs.appendFileSync(path.join(process.cwd(), 'yarn-invocations.txt'), `${args.join(' ')}\n`)
  console.log(`yarn ${args.join(' ')}`)
}
