import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('pnpm sbom --sbom-format cyclonedx outputs valid JSON to stdout', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  await execPnpm(['install'])

  const { status, stdout } = execPnpmSync(['sbom', '--sbom-format', 'cyclonedx'])

  expect(status).toBe(0)

  const parsed = JSON.parse(stdout.toString())
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.specVersion).toBe('1.7')
  expect(parsed.components.length).toBeGreaterThan(0)
})

test('pnpm sbom --sbom-format spdx outputs valid JSON to stdout', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  await execPnpm(['install'])

  const { status, stdout } = execPnpmSync(['sbom', '--sbom-format', 'spdx'])

  expect(status).toBe(0)

  const parsed = JSON.parse(stdout.toString())
  expect(parsed.spdxVersion).toBe('SPDX-2.3')
  expect(parsed.dataLicense).toBe('CC0-1.0')
})

test('pnpm sbom warnings go to stderr, not stdout', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  await execPnpm(['install'])

  // pnpm_config_force triggers a WARN log; disable silent mode so the reporter runs
  const { status, stdout, stderr } = execPnpmSync(
    ['sbom', '--sbom-format', 'cyclonedx'],
    { env: { pnpm_config_silent: 'false', pnpm_config_force: 'true' } }
  )

  expect(status).toBe(0)

  // stdout must still be valid JSON
  const parsed = JSON.parse(stdout.toString())
  expect(parsed.bomFormat).toBe('CycloneDX')

  // the --force warning should be on stderr, not stdout
  expect(stderr.toString()).toContain('using --force')
})

test('pnpm sbom reads dedicated workspace lockfiles', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0', private: true } },
    { location: 'packages/a', package: { name: 'pkg-a', version: '1.0.0', dependencies: { 'is-positive': '3.1.0', 'pkg-b': 'workspace:*' } } },
    { location: 'packages/b', package: { name: 'pkg-b', version: '1.0.0', dependencies: { 'is-number': '6.0.0' } } },
  ])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n')
  await execPnpm(['install'])

  const filtered = execPnpmSync(['--filter', 'pkg-a', 'sbom', '--sbom-format', 'spdx'])
  expect(filtered.status).toBe(0)
  const filteredPackages = JSON.parse(filtered.stdout.toString()).packages.map((pkg: { name: string }) => pkg.name)
  expect(filteredPackages).toContain('pkg-a')
  expect(filteredPackages).toContain('pkg-b')
  expect(filteredPackages).toContain('is-positive')
  expect(filteredPackages).toContain('is-number')

  const unfiltered = execPnpmSync(['sbom', '--sbom-format', 'spdx'])
  expect(unfiltered.status).toBe(0)
  expect(JSON.parse(unfiltered.stdout.toString()).packages.map((pkg: { name: string }) => pkg.name)).toContain('is-number')

  const recursive = execPnpmSync(['-r', 'sbom', '--sbom-format', 'spdx'])
  expect(recursive.status).toBe(0)
  expect(JSON.parse(recursive.stdout.toString()).packages.map((pkg: { name: string }) => pkg.name)).toContain('is-number')

  const split = execPnpmSync(['sbom', '--sbom-format', 'spdx', '--split'])
  expect(split.status).toBe(0)
  const documents = split.stdout.toString().trim().split('\n').map((line) => JSON.parse(line))
  expect(documents.map((document) => document.name).sort()).toStrictEqual(['pkg-a', 'pkg-b', 'root'])
  const appSbom = documents.find((document) => document.name === 'pkg-a')
  expect(appSbom.packages.map((pkg: { name: string }) => pkg.name)).toContain('is-number')

  fs.rmSync(path.join('packages/a', 'pnpm-lock.yaml'))
  const missing = execPnpmSync(['--filter', 'pkg-a', 'sbom', '--sbom-format', 'spdx'])
  expect(missing.status).toBe(1)
  expect(missing.stderr.toString()).toContain('ERR_PNPM_SBOM_MISSING_IMPORTERS')
})
