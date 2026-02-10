/// <reference path="../../../__typings__/index.d.ts" />
import path from 'path'
import { STORE_VERSION } from '@pnpm/constants'
import { sbom } from '@pnpm/plugin-commands-sbom'
import { install } from '@pnpm/plugin-commands-installation'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

test('pnpm sbom --sbom-format cyclonedx', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.specVersion).toBe('1.7')
  expect(parsed.metadata.component.name).toBe('simple-sbom-test')
  expect(parsed.components.length).toBeGreaterThan(0)

  const isPositive = parsed.components.find(
    (c: { name: string }) => c.name === 'is-positive'
  )
  expect(isPositive).toBeDefined()
  expect(isPositive.purl).toBe('pkg:npm/is-positive@3.1.0')
  expect(isPositive.version).toBe('3.1.0')
})

test('pnpm sbom --sbom-format spdx', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'spdx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.spdxVersion).toBe('SPDX-2.3')
  expect(parsed.dataLicense).toBe('CC0-1.0')
  expect(parsed.packages.length).toBeGreaterThanOrEqual(2)

  const isPositive = parsed.packages.find(
    (p: { name: string }) => p.name === 'is-positive'
  )
  expect(isPositive).toBeDefined()
  expect(isPositive.externalRefs[0].referenceLocator).toBe('pkg:npm/is-positive@3.1.0')
})

test('pnpm sbom --lockfile-only', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  // No install — just lockfile
  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.components.length).toBeGreaterThan(0)

  const isPositive = parsed.components.find(
    (c: { name: string }) => c.name === 'is-positive'
  )
  expect(isPositive).toBeDefined()
  // In lockfile-only mode, license metadata is absent
  expect(isPositive.licenses).toBeUndefined()
})

test('pnpm sbom missing --sbom-format throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
    })
  ).rejects.toThrow('--sbom-format option is required')
})

test('pnpm sbom invalid --sbom-format throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'invalid',
    })
  ).rejects.toThrow('Invalid SBOM format')
})

test('pnpm sbom with missing lockfile throws', async () => {
  const workspaceDir = tempDir()

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
    })
  ).rejects.toThrow('Cannot generate SBOM without a lockfile')
})

test('pnpm sbom --prod excludes devDependencies', async () => {
  const workspaceDir = tempDir()
  f.copy('with-dev-dependency', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    production: true,
    dev: false,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  expect(componentNames).not.toContain('typescript')
})

test('pnpm sbom invalid --sbom-type throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
      sbomType: 'invalid',
      lockfileOnly: true,
    })
  ).rejects.toThrow('Invalid SBOM type')
})

test('pnpm sbom --sbom-type application', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    sbomType: 'application',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.metadata.component.type).toBe('application')
})
