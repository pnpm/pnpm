import fs from 'fs'
import path from 'path'
import { type ProjectManifest } from '@pnpm/types'
import { add, install } from '@pnpm/plugin-commands-installation'
import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { DEFAULT_OPTS } from './utils'

const f = fixtures(__dirname)

function addPatch (key: string, patchFixture: string, patchDest: string): void {
  fs.mkdirSync(path.dirname(patchDest), { recursive: true })
  fs.copyFileSync(patchFixture, patchDest)
  let manifestText = fs.readFileSync('package.json', 'utf-8')
  const manifest: ProjectManifest = JSON.parse(manifestText)
  manifest.pnpm = {
    ...manifest.pnpm,
    patchedDependencies: {
      ...manifest.pnpm?.patchedDependencies,
      [key]: patchDest,
    },
  }
  manifestText = JSON.stringify(manifest, undefined, 2) + '\n'
  fs.writeFileSync('package.json', manifestText)
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

  addPatch('@pnpm.e2e/console-log', patchFixture, 'patches/console-log.patch')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
  })

  {
    const text = patchedFileContent(1)
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text.trim().split('\n').length).toBe(2)
  }

  {
    const text = patchedFileContent(2)
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('third line')
    expect(text.trim().split('\n').length).toBe(3)
  }

  {
    const text = patchedFileContent(3)
    expect(text).toContain('FIRST LINE')
    expect(text).not.toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('third line')
    expect(text).toContain('fourth line')
    expect(text.trim().split('\n').length).toBe(4)
  }
})

test('bare package name as a patchedDependencies key should apply to all possible versions and skip non-applicable versions', async () => {
  const patchFixture = f.find('patchedDependencies/console-log-replace-3rd-line.patch')
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/depends-on-console-log@1.0.0'])

  addPatch('@pnpm.e2e/console-log', patchFixture, 'patches/console-log.patch')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    frozenLockfile: false,
  })

  // the common patch does not apply to v1
  expect(patchedFileContent(1)).toBe(unpatchedFileContent(1))

  // the common patch applies to v2
  {
    const text = patchedFileContent(2)
    expect(text).not.toBe(unpatchedFileContent(2))
    expect(text).toContain('first line')
    expect(text).toContain('second line')
    expect(text).toContain('THIRD LINE')
    expect(text).not.toContain('third line')
    expect(text.trim().split('\n').length).toBe(3)
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
    expect(text.trim().split('\n').length).toBe(4)
  }
})
