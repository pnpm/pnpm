#!/usr/bin/env node
const [major, minor] = process.version.substr(1).split('.')
const COMPATIBILITY_PAGE = `Visit https://r.pnpm.io/comp to see the list of past pnpm versions with respective Node.js version support.`

if (major < 12 || major == 12 && minor < 17) {
  console.log(`ERROR: This version of pnpm requires at least Node.js v12.17
The current version of Node.js is ${process.version}
${COMPATIBILITY_PAGE}`)
  process.exit(1)
} else if (major == 13 && minor < 7) {
  console.log(`ERROR: This version of pnpm requires at least Node.js v13.7
The current version of Node.js is ${process.version}
${COMPATIBILITY_PAGE}`)
  process.exit(1)
}

require('../dist/pnpm.cjs')

// if you want to debug at your local env, you can use this
// require('../lib/pnpm')