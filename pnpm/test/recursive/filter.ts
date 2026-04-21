import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpmSync } from '../utils/index.js'

test('pnpm --filter <root> add <pkg> should work', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
  })

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "."\noverrides:\n  is-positive: "1.0.0"\n')

  const result = execPnpmSync(['--filter', 'root', 'add', 'is-positive'])
  if (result.status !== 0) {
    console.log(result.stdout.toString())
    console.log(result.stderr.toString())
  }
  expect(result.status).toBe(0)

  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  expect(pkg.dependencies['is-positive']).toBeTruthy()
})

test('pnpm --filter . add <pkg> should work', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
  })

  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "."\n')

  const result = execPnpmSync(['--filter', '.', 'add', 'is-positive'])
  if (result.status !== 0) {
    console.log(result.stdout.toString())
    console.log(result.stderr.toString())
  }
  expect(result.status).toBe(0)

  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  expect(pkg.dependencies['is-positive']).toBeTruthy()
})
