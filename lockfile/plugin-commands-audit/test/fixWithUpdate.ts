import { join } from 'path'
import { readFile } from 'fs/promises'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { filterPackagesFromDir } from '@pnpm/filter-workspace-packages'
import { type DepPath } from '@pnpm/types'
import { addDistTag } from '@pnpm/registry-mock'
import chalk from 'chalk'
import nock from 'nock'
import { MOCK_REGISTRY, MOCK_REGISTRY_OPTS } from './utils/options.js'

const f = fixtures(import.meta.dirname)

describe('audit fix with update', () => {
  afterEach(() => nock.cleanAll())
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

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)
    expect(exitCode).toBe(0)

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

  test('top-level pinned vulnerability is fixed by updating the vulnerable package', async () => {
    const tmp = f.prepare('update-single-pinned')

    const originalPkgId = '@pnpm.e2e/pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/pkg-with-1-dep@100.1.0' as DepPath

    const originalDepPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' as DepPath
    const expectedDepPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' as DepPath

    const { manifest: originalManifest } = await readProjectManifest(tmp)
    expect(originalManifest).toBeTruthy()
    expect(originalManifest.dependencies).toBeDefined()
    expect(originalManifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()
    expect(originalLockfile!.packages![originalDepPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedDepPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'top-level-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)
    expect(exitCode).toBe(0)

    const { manifest } = await readProjectManifest(tmp)
    expect(manifest).toBeTruthy()
    expect(manifest.dependencies).toBeDefined()
    expect(manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('100.1.0')

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // The vulnerable dependency should be updated
    expect(packagesArray).not.toContain(originalPkgId)
    expect(packagesArray).toContain(expectedPkgId)

    // The vulnerable dependency's dependencies should also be updated
    expect(packagesArray).not.toContain(originalDepPkgId)
    expect(packagesArray).toContain(expectedDepPkgId)

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId) continue
      if (pkgId === originalDepPkgId) continue
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

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/dep-of-pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/dep-of-pkg-with-1-dep')}
`)
    expect(exitCode).toBe(0)

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

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/dep-of-pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/dep-of-pkg-with-1-dep')}
`)
    expect(exitCode).toBe(0)

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

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(0)} vulnerabilities were fixed, ${chalk.red(1)} vulnerability remains.

The remaining vulnerabilities are:
- (${chalk.bold.red('high')}) "${chalk.bold.red('Title: unfixable vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)
    expect(exitCode).toBe(1)

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

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(2)} vulnerabilities were fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('critical')}) "${chalk.green('form-data uses unsafe random function in form-data for choosing boundary')}" ${chalk.blue('form-data')}
- (${chalk.green('critical')}) "${chalk.green('form-data uses unsafe random function in form-data for choosing boundary')}" ${chalk.blue('form-data')}
`)
    expect(exitCode).toBe(0)

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

  test('top-level workspace subpackage vulnerability is fixed by recursive update from root', async () => {
    const tmp = f.prepare('update-workspace-depth-2')

    const originalPkgId = '@pnpm.e2e/pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/pkg-with-1-dep@100.1.0' as DepPath

    const subPkgDir = join(tmp, 'packages', 'sub-pkg')

    const { manifest: originalManifest } = await readProjectManifest(subPkgDir)
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

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const {
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
    } = await filterPackagesFromDir(tmp, [], {
      workspaceDir: tmp,
      prefix: tmp,
    })
    expect(allProjects).toHaveLength(2)
    expect(new Set(allProjects.map(p => p.manifest.name))).toEqual(new Set(['update-workspace-depth-2', 'sub-pkg']))
    expect(allProjectsGraph).toBeTruthy()
    expect(selectedProjectsGraph).toEqual(allProjectsGraph)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      workspaceDir: tmp,
      lockfileDir: tmp,
      rootProjectManifestDir: tmp,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)
    expect(exitCode).toBe(0)

    const { manifest } = await readProjectManifest(subPkgDir)
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

  test('depth 2 workspace subpackage vulnerability is fixed by recursive update from root', async () => {
    const tmp = f.prepare('update-workspace-depth-2')

    const originalPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' as DepPath

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'depth-2-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const {
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
    } = await filterPackagesFromDir(tmp, [], {
      workspaceDir: tmp,
      prefix: tmp,
    })
    expect(allProjects).toHaveLength(2)
    expect(new Set(allProjects.map(p => p.manifest.name))).toEqual(new Set(['update-workspace-depth-2', 'sub-pkg']))
    expect(allProjectsGraph).toBeTruthy()
    expect(selectedProjectsGraph).toEqual(allProjectsGraph)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      workspaceDir: tmp,
      lockfileDir: tmp,
      rootProjectManifestDir: tmp,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
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

  test('top-level pinned workspace subpackage vulnerability is fixed by recursive update from root', async () => {
    const tmp = f.prepare('update-workspace-pinned')

    const originalPkgId = '@pnpm.e2e/pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/pkg-with-1-dep@100.1.0' as DepPath

    const originalDepPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' as DepPath
    const expectedDepPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' as DepPath

    const subPkgDir = join(tmp, 'packages', 'sub-pkg')

    const { manifest: originalManifest } = await readProjectManifest(subPkgDir)
    expect(originalManifest).toBeTruthy()
    expect(originalManifest.dependencies).toBeDefined()
    expect(originalManifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()
    expect(originalLockfile!.packages![originalDepPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedDepPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'top-level-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(MOCK_REGISTRY, { allowUnmocked: true })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, mockResponse)

    const {
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
    } = await filterPackagesFromDir(tmp, [], {
      workspaceDir: tmp,
      prefix: tmp,
    })
    expect(allProjects).toHaveLength(2)
    expect(new Set(allProjects.map(p => p.manifest.name))).toEqual(new Set(['update-workspace-pinned', 'sub-pkg']))
    expect(allProjectsGraph).toBeTruthy()
    expect(selectedProjectsGraph).toEqual(allProjectsGraph)

    const { exitCode, output } = await audit.handler({
      ...MOCK_REGISTRY_OPTS,
      dir: tmp,
      workspaceDir: tmp,
      lockfileDir: tmp,
      rootProjectManifestDir: tmp,
      allProjects,
      allProjectsGraph,
      selectedProjectsGraph,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(output).toBe(`${chalk.green(1)} vulnerability was fixed, ${chalk.red(0)} vulnerabilities remain.

The fixed vulnerabilities are:
- (${chalk.green('high')}) "${chalk.green('Title: mock vulnerability in @pnpm.e2e/pkg-with-1-dep')}" ${chalk.blue('@pnpm.e2e/pkg-with-1-dep')}
`)
    expect(exitCode).toBe(0)

    const { manifest } = await readProjectManifest(subPkgDir)
    expect(manifest).toBeTruthy()
    expect(manifest.dependencies).toBeDefined()
    expect(manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBe('100.1.0')

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()
    const packagesArray = Object.keys(lockfile!.packages!)

    // The vulnerable dependency should be updated
    expect(packagesArray).not.toContain(originalPkgId)
    expect(packagesArray).toContain(expectedPkgId)

    // The vulnerable dependency's dependencies should also be updated
    expect(packagesArray).not.toContain(originalDepPkgId)
    expect(packagesArray).toContain(expectedDepPkgId)

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId) continue
      if (pkgId === originalDepPkgId) continue
      expect(packagesArray).toContain(pkgId)
    }
  })
})
