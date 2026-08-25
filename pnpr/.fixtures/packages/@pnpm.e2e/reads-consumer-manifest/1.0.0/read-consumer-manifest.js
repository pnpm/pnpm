'use strict'
// The convention a git-hook installer follows to find the project that
// installed it: walk out of its own directory to the `node_modules` holding
// it, and read the manifest of whatever sits above that. Nothing promises a
// manifest is there, and this package does not care what it says — it only
// has to be readable, which is what pnpm/pnpm#13318 was about.
const fs = require('fs')
const path = require('path')

let dir = process.cwd()
while (path.basename(path.dirname(dir)) !== 'node_modules') {
  const parent = path.dirname(dir)
  if (parent === dir) throw new Error('no node_modules above ' + process.cwd())
  dir = parent
}
const assumedProjectDir = path.dirname(path.dirname(dir))

fs.statSync(path.join(assumedProjectDir, 'package.json'))
fs.writeFileSync('read-consumer-manifest-from.txt', assumedProjectDir, 'utf8')
