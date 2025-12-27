import fs from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { jest } from '@jest/globals'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils/index.js'

const originalModule = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => {
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

const { globalWarn } = await import('@pnpm/logger')
const { add, install } = await import('@pnpm/plugin-commands-installation')

const f = fixtures(import.meta.dirname)
// Track cumulative patchedDependencies across multiple addPatch calls
let currentPatchedDependencies: Record<string, string> = {}

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
  currentPatchedDependencies = {}
})

function addPatch (key: string, patchFixture: string, patchDest: string): Record<string, string> {
  fs.mkdirSync(path.dirname(patchDest), { recursive: true })
  fs.copyFileSync(patchFixture, patchDest)

  currentPatchedDependencies = {
    ...currentPatchedDependencies,
    [key]: patchDest,
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['.'],
    patchedDependencies: currentPatchedDependencies,
  })

  return currentPatchedDependencies
}

const unpatchedModulesDir = (v: 1 | 2 | 3) => `node_modules/.pnpm/@pnpm.e2e+console-log@${v}.0.0/node_modules`
const unpatchedFilePath = (v: 1 | 2 | 3) => `${unpatchedModulesDir(v)}/@pnpm.e2e/console-log/index.js`
const unpatchedFileContent = (v: 1 | 2 | 3) => fs.readFileSync(unpatchedFilePath(v), 'utf-8')
const patchedModulesDir = 'node_modules/.pnpm/@pnpm.e2e+depends-on-console-log@1.0.0/node_modules'
const patchedFilePath = (v: 1 | 2 | 3) => `${patchedModulesDir}/console-log-${v}/index.js`
const patchedFileContent = (v: 1 | 2 | 3) => fs.readFileSync(patchedFilePath(v), 'utf-8')

test('bare package name as a patchedDependencies key should apply to all versions if all are applicable', async () => {
  const patchFixture = f.find('patchedDependencies/console-log-replace-1st-line.patch')
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/depends-on-console-log@1.0.0'])
  fs.rmSync('pnpm-lock.yaml')

  const patchedDependencies = addPatch('@pnpm.e2e/console-log', patchFixture, 'patches/console-log.patch')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
    patchedDependencies,
  })

  {
    const text = patchedFileContent(1)
    expect(text).not.toBe(unpatchedFileContent(1))
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text.trim().split('\n')).toHaveLength(2)
  }

  {
    const text = patchedFileContent(2)
    expect(text).not.toBe(unpatchedFileContent(2))
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('third line')
    expect(text.trim().split('\n')).toHaveLength(3)
  }

  {
    const text = patchedFileContent(3)
    expect(text).not.toBe(unpatchedFileContent(3))
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('third line')
    expect(text).toContain('fourth line')
    expect(text.trim().split('\n')).toHaveLength(4)
  }

  expect(globalWarn).not.toHaveBeenCalledWith(expect.stringContaining('Could not apply patch'))
})

test('bare package name as a patchedDependencies key should apply to all possible versions and skip non-applicable versions', async () => {
  const patchFixture = f.find('patchedDependencies/console-log-replace-3rd-line.patch')
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/depends-on-console-log@1.0.0'])
  fs.rmSync('pnpm-lock.yaml')

  const patchedDependencies = addPatch('@pnpm.e2e/console-log', patchFixture, 'patches/console-log.patch')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
    patchedDependencies,
  })

  // the common patch does not apply to v1
  expect(patchedFileContent(1)).toBe(unpatchedFileContent(1))
  expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining(`Could not apply patch ${path.resolve('patches/console-log.patch')}`))

  // the common patch applies to v2
  {
    const text = patchedFileContent(2)
    expect(text).not.toBe(unpatchedFileContent(2))
    expect(text).toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('THIRD LINE')
    expect(text).not.toContain('third line')
    expect(text.trim().split('\n')).toHaveLength(3)
  }

  // the common patch applies to v3
  {
    const text = patchedFileContent(3)
    expect(text).not.toBe(unpatchedFileContent(3))
    expect(text).toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('THIRD LINE')
    expect(text).not.toContain('third line')
    expect(text).toContain('fourth line')
    expect(text.trim().split('\n')).toHaveLength(4)
  }
})

test('package name with version is prioritized over bare package name as keys of patchedDependencies', async () => {
  const commonPatchFixture = f.find('patchedDependencies/console-log-replace-1st-line.patch')
  const specializedPatchFixture = f.find('patchedDependencies/console-log-replace-2nd-line.patch')
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/depends-on-console-log@1.0.0'])
  fs.rmSync('pnpm-lock.yaml')

  addPatch('@pnpm.e2e/console-log', commonPatchFixture, 'patches/console-log.patch')
  const patchedDependencies = addPatch('@pnpm.e2e/console-log@2.0.0', specializedPatchFixture, 'patches/console-log@2.0.0.patch')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
    patchedDependencies,
  })

  // the common patch applies to v1
  {
    const text = patchedFileContent(1)
    expect(text).not.toBe(unpatchedFileContent(1))
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text.trim().split('\n')).toHaveLength(2)
  }

  // the specialized patch applies to v2
  {
    const text = patchedFileContent(2)
    expect(text).not.toBe(unpatchedFileContent(2))
    expect(text).toContain('first line')
    expect(text).toContain('SECOND LINE')
    expect(text).not.toContain('second line')
    expect(text).toContain('third line')
    expect(text.trim().split('\n')).toHaveLength(3)
  }

  // the common patch applies to v3
  {
    const text = patchedFileContent(3)
    expect(text).not.toBe(unpatchedFileContent(3))
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('third line')
    expect(text).toContain('fourth line')
    expect(text.trim().split('\n')).toHaveLength(4)
  }

  expect(globalWarn).not.toHaveBeenCalledWith(expect.stringContaining('Could not apply patch'))
})

test('package name with version as a patchedDependencies key does not affect other versions', async () => {
  const patchFixture2 = f.find('patchedDependencies/console-log-replace-2nd-line.patch')
  const patchFixture3 = f.find('patchedDependencies/console-log-replace-4th-line.patch')
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/depends-on-console-log@1.0.0'])
  fs.rmSync('pnpm-lock.yaml')

  addPatch('@pnpm.e2e/console-log@2.0.0', patchFixture2, 'patches/console-log@2.0.0.patch')
  const patchedDependencies = addPatch('@pnpm.e2e/console-log@3.0.0', patchFixture3, 'patches/console-log@3.0.0.patch')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
    patchedDependencies,
  })

  // v1 remains unpatched
  expect(patchedFileContent(1)).toBe(unpatchedFileContent(1))

  // patch 2 applies to v2
  {
    const text = patchedFileContent(2)
    expect(text).not.toBe(unpatchedFileContent(2))
    expect(text).toContain('first line')
    expect(text).toContain('SECOND LINE')
    expect(text).not.toContain('second line')
    expect(text).toContain('third line')
    expect(text.trim().split('\n')).toHaveLength(3)
  }

  // patch 3 applies to v3
  {
    const text = patchedFileContent(3)
    expect(text).not.toBe(unpatchedFileContent(3))
    expect(text).toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('third line')
    expect(text).toContain('FOURTH LINE')
    expect(text).not.toContain('fourth line')
    expect(text.trim().split('\n')).toHaveLength(4)
  }
})

test('failure to apply patch with package name and version would cause throw an error', async () => {
  const patchFixture = f.find('patchedDependencies/console-log-replace-4th-line.patch')
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/depends-on-console-log@1.0.0'])
  fs.rmSync('pnpm-lock.yaml')

  const patchedDependencies = addPatch('@pnpm.e2e/console-log@1.0.0', patchFixture, 'patches/console-log@1.0.0.patch')

  const promise = install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
    patchedDependencies,
  })
  await expect(promise).rejects.toHaveProperty(['message'], expect.stringContaining('Could not apply patch'))
  await expect(promise).rejects.toHaveProperty(['message'], expect.stringContaining(path.resolve('patches/console-log@1.0.0.patch')))

  expect(patchedFileContent(1)).toBe(unpatchedFileContent(1))
})
