'use strict'

const platformDependency = require('@pnpm.e2e/only-linux-x64-glibc/package.json')

process.stdout.write(`${platformDependency.name}\n`)
