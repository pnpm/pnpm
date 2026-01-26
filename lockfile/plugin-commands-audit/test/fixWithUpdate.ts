import { join } from 'path'
import { readFile } from 'fs/promises'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { type DepPath } from '@pnpm/types'
import { REGISTRY_MOCK_PORT, addDistTag } from '@pnpm/registry-mock'
import chalk from 'chalk'
import nock from 'nock'
import { DEFAULT_OPTS, AUDIT_REGISTRY } from './utils/options.js'

const f = fixtures(import.meta.dirname)

const registry = `http://localhost:${REGISTRY_MOCK_PORT}`

describe('audit fix with update', () => {
  test('top-level vulnerability is fixed by updating the vulnerable package', async () => {
    const tmp = f.prepare('update-single-depth-2')

    const originalPkgId = '@pnpm.e2e/pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/pkg-with-1-dep@100.1.0' as DepPath

    const { manifest: originalManifest } = await readProjectManifest(tmp)
    expect(originalManifest).toBeTruthy()
    expect(originalManifest.dependencies).toBeDefined()
    expect(originalManifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('^100.0.0')

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'top-level-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: tmp,
      registries: { default: registry },
      rawConfig: { registry },
      userConfig: {},
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(exitCode).toBe(0)
    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)

    const { manifest } = await readProjectManifest(tmp)
    expect(manifest).toBeTruthy()
    expect(manifest.dependencies).toBeDefined()
    expect(manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('^100.1.0')

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // The vulnerable dependency should be updated
    expect(packagesArray).not.toContain(originalPkgId)
    expect(packagesArray).toContain(expectedPkgId)

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId) continue
      expect(packagesArray).toContain(pkgId)
    }
  })

  test('depth 2 vulnerability is fixed by updating the vulnerable package', async () => {
    const tmp = f.prepare('update-single-depth-2')

    const originalPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' as DepPath

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'depth-2-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: tmp,
      registries: { default: registry },
      rawConfig: { registry },
      userConfig: {},
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(exitCode).toBe(0)
    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/dep-of-pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/dep-of-pkg-with-1-dep')}
`)

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // The vulnerable dependency should be updated
    expect(packagesArray).not.toContain(originalPkgId)
    expect(packagesArray).toContain(expectedPkgId)

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId) continue
      expect(packagesArray).toContain(pkgId)
    }
  })

  test('depth 3 vulnerability is fixed by updating the vulnerable package', async () => {
    const tmp = f.prepare('update-single-depth-3')

    const originalPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' as DepPath

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'depth-3-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: tmp,
      registries: { default: registry },
      rawConfig: { registry },
      userConfig: {},
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(exitCode).toBe(0)
    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/dep-of-pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/dep-of-pkg-with-1-dep')}
`)

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // The vulnerable dependency should be updated
    expect(packagesArray).not.toContain(originalPkgId)
    expect(packagesArray).toContain(expectedPkgId)

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId) continue
      expect(packagesArray).toContain(pkgId)
    }
  })

  test('unfixable vulnerability remains unresolved', async () => {
    const tmp = f.prepare('update-single-depth-2')

    const pkgId = '@pnpm.e2e/pkg-with-1-dep@100.0.0' as DepPath

    const { manifest: originalManifest } = await readProjectManifest(tmp)
    expect(originalManifest).toBeTruthy()
    expect(originalManifest.dependencies).toBeDefined()
    expect(originalManifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('^100.0.0')

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![pkgId]).toBeDefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'unfixable-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: tmp,
      registries: { default: registry },
      rawConfig: { registry },
      userConfig: {},
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(exitCode).toBe(0)
    expect(output).toBe(`${chalk.green(0)} vulnerabilities were fixed, ${chalk.red(1)} vulnerability remains.

The remaining vulnerabilities are:
- (${chalk.bold.red('high')}) "${chalk.bold.red('Title: unfixable vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)

    // The manifest should remain unchanged
    const { manifest } = await readProjectManifest(tmp)
    expect(manifest).toBeTruthy()
    expect(manifest.dependencies).toBeDefined()
    expect(manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('^100.0.0')

    // The lockfile should remain unchanged
    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // All packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      expect(packagesArray).toContain(pkgId)
    }
  })

  test('vulnerable package with multiple versions is updated', async () => {
    await addDistTag({ package: 'form-data', version: '4.0.4', distTag: 'latest' })

    const tmp = f.prepare('update-multiple')

    const originalPkgId1 = 'form-data@3.0.1' as DepPath
    const originalPkgId2 = 'form-data@4.0.0' as DepPath
    const expectedPkgId1 = 'form-data@3.0.4' as DepPath
    const expectedPkgId2 = 'form-data@4.0.4' as DepPath

    const { manifest: originalManifest } = await readProjectManifest(tmp)
    expect(originalManifest).toBeTruthy()
    expect(originalManifest.dependencies).toBeDefined()
    expect(originalManifest.dependencies?.['form-data']).toBe('^3.0.1')

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId1]).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId2]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId1]).toBeUndefined()
    expect(originalLockfile!.packages![expectedPkgId2]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'form-data-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: tmp,
      registries: { default: registry },
      rawConfig: { registry },
      userConfig: {},
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(exitCode).toBe(0)
    expect(output).toBe(`${chalk.green(2)} vulnerabilities were fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('critical')}) "${chalk.green('form-data uses unsafe random function in form-data for choosing boundary')}" ${chalk.blue('form-data')}
- (${chalk.green('critical')}) "${chalk.green('form-data uses unsafe random function in form-data for choosing boundary')}" ${chalk.blue('form-data')}
`)

    const { manifest } = await readProjectManifest(tmp)
    expect(manifest).toBeTruthy()
    expect(manifest.dependencies).toBeDefined()
    expect(manifest.dependencies?.['form-data']).toBe('^3.0.4')

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // The vulnerable dependency should be updated
    expect(packagesArray).not.toContain(originalPkgId1)
    expect(packagesArray).not.toContain(originalPkgId2)
    expect(packagesArray).toContain(expectedPkgId1)
    expect(packagesArray).toContain(expectedPkgId2)

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId1 || pkgId === originalPkgId2) continue
      expect(packagesArray).toContain(pkgId)
    }
  })
})
