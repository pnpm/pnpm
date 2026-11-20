#!/usr/bin/env node
'use strict'

// A dependency of a dependency, reachable only through the hoisted modules dir.
const { name } = require('@pnpm.e2e/dep-of-pkg-with-1-dep/package.json')

process.stdout.write(`${name}\n`)
