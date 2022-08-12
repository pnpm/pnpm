#!/usr/bin/env node
const semver = require('semver')

const COMPATIBILITY_PAGE = `Visit https://r.pnpm.io/comp to see the list of past pnpm versions with respective Node.js version support.`

if (!semver.satisfies(process.version, '^12.17.0 || >=13.7.0')) {
  console.log(`ERROR: This version of pnpm requires at least Node.js v12.17 or v13.7
The current version of Node.js is ${process.version}
${COMPATIBILITY_PAGE}`)
  process.exit(1)
}

process.argv = [...process.argv.slice(0, 2), 'dlx', ...process.argv.slice(2)]

require('../dist/pnpm.cjs')
