import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test as jestTest } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// The fake provider is an executable script with a Unix shebang, which
// child_process.spawn cannot execute on Windows.
const test = isWindows() ? jestTest.skip : jestTest

// The minimal protocol-v1 provider: materializes every requested depPath
// as a directory whose node_modules holds a stub of the package.
const FAKE_PROVIDER = `#!/usr/bin/env node
const fs = require('fs')
const path = require('path')
let input = ''
process.stdin.on('data', (chunk) => { input += chunk })
process.stdin.on('end', () => {
  const request = JSON.parse(input)
  const subdir = (depPath) => depPath.replace(/[^A-Za-z0-9._@-]/g, '+')
  const paths = {}
  for (const [depPath, node] of Object.entries(request.nodes)) {
    const dir = path.join(__dirname, 'store', subdir(depPath))
    fs.mkdirSync(path.join(dir, 'node_modules', node.name), { recursive: true })
    fs.writeFileSync(path.join(dir, 'node_modules', node.name, 'package.json'), JSON.stringify({ name: node.name, version: node.version }))
    paths[depPath] = dir
  }
  process.stdout.write(JSON.stringify({ protocol: 1, paths, skipped: [] }))
})
`

test('the CLI reads packageProvider from pnpm-workspace.yaml and materializes through it', async () => {
  prepare()
  const providerDir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-package-provider-'))
  const providerBin = path.join(providerDir, 'provider.js')
  fs.writeFileSync(providerBin, FAKE_PROVIDER, { mode: 0o755 })
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    packageProvider: providerBin,
  })

  await execPnpm(['add', 'is-positive@1.0.0'])

  const link = fs.readlinkSync(path.join('node_modules', 'is-positive'))
  expect(path.isAbsolute(link)).toBeTruthy()
  const realDir = fs.realpathSync(path.join('node_modules', 'is-positive'))
  expect(realDir.startsWith(path.join(providerDir, 'store'))).toBeTruthy()
})
