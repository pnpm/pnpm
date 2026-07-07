import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'

import { testDefaults } from './utils/index.js'

const f = fixtures(import.meta.dirname)

// Mimics the nix-provider contract: materializes every requested depPath as
// a directory whose node_modules holds the package next to symlinks to its
// dependencies, and records the request for assertions.
const FAKE_PROVIDER = `#!/usr/bin/env node
const fs = require('fs')
const path = require('path')
let input = ''
process.stdin.on('data', (chunk) => { input += chunk })
process.stdin.on('end', () => {
  const request = JSON.parse(input)
  fs.writeFileSync(path.join(__dirname, 'request.json'), JSON.stringify(request))
  const subdir = (depPath) => depPath.replace(/[^A-Za-z0-9._@-]/g, '+')
  const paths = {}
  for (const [depPath, node] of Object.entries(request.nodes)) {
    const dir = path.join(__dirname, 'store', subdir(depPath))
    fs.mkdirSync(path.join(dir, 'node_modules', node.name), { recursive: true })
    fs.writeFileSync(path.join(dir, 'node_modules', node.name, 'package.json'), JSON.stringify({ name: node.name, version: node.version }))
    paths[depPath] = dir
  }
  for (const [depPath, node] of Object.entries(request.nodes)) {
    for (const [alias, dep] of Object.entries(node.deps)) {
      const link = path.join(paths[depPath], 'node_modules', alias)
      fs.mkdirSync(path.dirname(link), { recursive: true })
      fs.symlinkSync(path.join(paths[dep.depPath], 'node_modules', dep.name), link)
    }
  }
  process.stdout.write(JSON.stringify({ protocol: 1, paths }))
})
`

function prepareFakeProvider (script: string = FAKE_PROVIDER): { providerBin: string, providerDir: string } {
  const providerDir = fs.mkdtempSync(path.join(os.tmpdir(), 'fake-package-provider-'))
  const providerBin = path.join(providerDir, 'provider.js')
  fs.writeFileSync(providerBin, script, { mode: 0o755 })
  return { providerBin, providerDir }
}

test('packages are materialized through the package provider and symlinked from its directories', async () => {
  const project = prepareEmpty()
  const { providerBin, providerDir } = prepareFakeProvider()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ packageProvider: providerBin }))

  const manifest = project.requireModule('@pnpm.e2e/pkg-with-1-dep/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/pkg-with-1-dep')

  // the direct dependency resolves into the provider's store
  const realDir = fs.realpathSync(path.join('node_modules', '@pnpm.e2e/pkg-with-1-dep'))
  expect(realDir.startsWith(path.join(providerDir, 'store'))).toBeTruthy()

  // the transitive dependency is reachable as a sibling inside the provider's store
  const transitive = fs.realpathSync(path.join(realDir, '../..', '@pnpm.e2e/dep-of-pkg-with-1-dep'))
  expect(transitive.startsWith(path.join(providerDir, 'store'))).toBeTruthy()

  // the provider received a closed graph with resolutions and the gc root location
  const request = JSON.parse(fs.readFileSync(path.join(providerDir, 'request.json'), 'utf8'))
  expect(request.protocol).toBe(1)
  expect(request.gcRootDir).toBe(path.join(process.cwd(), 'node_modules', '.pnpm-nix'))
  const requestedNodes = Object.values<any>(request.nodes) // eslint-disable-line @typescript-eslint/no-explicit-any
  const directNode = requestedNodes.find((node) => node.name === '@pnpm.e2e/pkg-with-1-dep')
  expect(directNode.tarball).toBeTruthy()
  expect(directNode.integrity).toMatch(/^sha/)
  expect(directNode.engine).toContain(process.platform)
  const depAlias = directNode.deps['@pnpm.e2e/dep-of-pkg-with-1-dep']
  expect(request.nodes[depAlias.depPath].name).toBe('@pnpm.e2e/dep-of-pkg-with-1-dep')

  // nothing was imported into the virtual store
  const virtualStoreEntries = fs.readdirSync(path.join('node_modules', '.pnpm'))
  expect(virtualStoreEntries.find((entry) => entry.includes('pkg-with-1-dep'))).toBeUndefined()

  // the lockfile is written as usual
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBeTruthy()
})

test('a repeat install keeps resolving from the package provider', async () => {
  const project = prepareEmpty()
  const { providerBin, providerDir } = prepareFakeProvider()
  const opts = testDefaults({ packageProvider: providerBin })

  const { updatedManifest } = await addDependenciesToPackage({}, ['is-positive'], opts)
  await addDependenciesToPackage(updatedManifest, ['is-negative'], opts)

  for (const pkgName of ['is-positive', 'is-negative']) {
    const realDir = fs.realpathSync(path.join('node_modules', pkgName))
    expect(realDir.startsWith(path.join(providerDir, 'store'))).toBeTruthy()
  }
  expect(project.requireModule('is-negative/package.json').name).toBe('is-negative')
})

test('patched dependencies are sent to the package provider with their patch content', async () => {
  prepareEmpty()
  const { providerBin, providerDir } = prepareFakeProvider()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  await install({
    dependencies: { 'is-positive': '1.0.0' },
  }, testDefaults({
    packageProvider: providerBin,
    patchedDependencies: { 'is-positive@1.0.0': patchPath },
  }))

  const request = JSON.parse(fs.readFileSync(path.join(providerDir, 'request.json'), 'utf8'))
  const node = Object.values<any>(request.nodes).find((requestNode) => requestNode.name === 'is-positive') // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(node.patch.content).toContain('// patched')
  expect(node.patch.hash).toBeTruthy()
})

test('the install aborts when the package provider fails', async () => {
  prepareEmpty()
  const { providerBin } = prepareFakeProvider('#!/usr/bin/env node\nprocess.exit(1)\n')

  await expect(
    addDependenciesToPackage({}, ['is-positive'], testDefaults({ packageProvider: providerBin }))
  ).rejects.toThrow(/package provider/)
})
