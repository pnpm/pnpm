import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { rimrafSync } from '@zkochan/rimraf'
import { loadJsonFileSync } from 'load-json-file'

import { execPnpm } from '../utils/index.js'

test('installing a bzip2 compressed tarball from URL', async () => {
  const project = prepareEmpty()
  const bz2Fixture = path.resolve(import.meta.dirname, '../../../store/cafs/test/fixtures/package.tar.bz2')
  const bz2Data = fs.readFileSync(bz2Fixture)

  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'application/x-bzip2' })
    res.end(bz2Data)
  })
  await new Promise<void>((resolve) => server.listen(0, resolve))
  const address = server.address()
  if (!address || typeof address === 'string') {
    server.close()
    throw new Error('Server address is invalid')
  }
  const url = `http://127.0.0.1:${address.port}/package.tar.bz2`

  try {
    await execPnpm(['add', url])
    expect(loadJsonFileSync<{ version: string }>('node_modules/test-bzip2-pkg/package.json').version).toBe('1.2.3')

    const lockfile = project.readLockfile()
    expect(lockfile.importers['.'].dependencies?.['test-bzip2-pkg']).toEqual({
      specifier: url,
      version: url,
    })

    rimrafSync('node_modules')
    await execPnpm(['install', '--frozen-lockfile'])
    expect(loadJsonFileSync<{ version: string }>('node_modules/test-bzip2-pkg/package.json').version).toBe('1.2.3')
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
})

test('installing a bzip2 compressed tarball from local file', async () => {
  const project = prepareEmpty()
  const bz2Fixture = path.resolve(import.meta.dirname, '../../../store/cafs/test/fixtures/package.tar.bz2')

  await execPnpm(['add', bz2Fixture])
  expect(loadJsonFileSync<{ version: string }>('node_modules/test-bzip2-pkg/package.json').version).toBe('1.2.3')

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['test-bzip2-pkg']).toBeDefined()

  rimrafSync('node_modules')
  await execPnpm(['install', '--frozen-lockfile'])
  expect(loadJsonFileSync<{ version: string }>('node_modules/test-bzip2-pkg/package.json').version).toBe('1.2.3')
})

test.each(['3.1.0', '1.0.0'])('adding a new unnamed tarball URL replaces the existing dependency with version %s', async (newVersion) => {
  const project = prepareEmpty()
  const tarballUrl = (version: string) => `http://127.0.0.1:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-${version}.tgz`
  const oldTarball = tarballUrl('1.0.0')
  const newTarball = `${tarballUrl(newVersion)}?revision=2`
  await execPnpm(['add', oldTarball])
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe('1.0.0')
  const oldPackageDir = fs.realpathSync('node_modules/is-positive')
  await execPnpm(['add', newTarball])

  expect(loadJsonFileSync<{ dependencies: Record<string, string> }>('package.json').dependencies['is-positive']).toBe(newTarball)
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe(newVersion)
  expect(fs.realpathSync('node_modules/is-positive')).not.toBe(oldPackageDir)
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-positive']).toEqual({
    specifier: newTarball,
    version: newTarball,
  })
  expect(lockfile.packages).toHaveProperty([`is-positive@${newTarball}`])
  expect(lockfile.packages).not.toHaveProperty([`is-positive@${oldTarball}`])

  rimrafSync('node_modules')
  await execPnpm(['install', '--frozen-lockfile'])
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe(newVersion)
  expect(project.readLockfile()).toEqual(lockfile)
})
