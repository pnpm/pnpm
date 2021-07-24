import fs from 'fs'
import path from 'path'
import { pack } from '@pnpm/plugin-commands-publishing'
import prepare, { tempDir } from '@pnpm/prepare'
import exists from 'path-exists'
import tar from 'tar'
import { DEFAULT_OPTS } from './utils'

test('pack: package with package.json', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })

  expect(await exists('test-publish-package.json-0.0.0.tgz')).toBeTruthy()
  expect(await exists('package.json')).toBeTruthy()
})

test('pack: package with package.yaml', async () => {
  prepare({
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })

  expect(await exists('test-publish-package.yaml-0.0.0.tgz')).toBeTruthy()
  expect(await exists('package.yaml')).toBeTruthy()
  expect(await exists('package.json')).toBeFalsy()
})

test('pack: package with package.json5', async () => {
  prepare({
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })

  expect(await exists('test-publish-package.json5-0.0.0.tgz')).toBeTruthy()
  expect(await exists('package.json5')).toBeTruthy()
  expect(await exists('package.json')).toBeFalsy()
})

test('pack a package with scoped name', async () => {
  prepare({
    name: '@pnpm/test-scope',
    version: '0.0.0',
  })

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })

  expect(await exists('pnpm-test-scope-0.0.0.tgz')).toBeTruthy()
})

test('pack: runs prepack, prepare, and postpack', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
    scripts: {
      prepack: 'node -e "require(\'fs\').writeFileSync(\'prepack\', \'\')"',
      prepare: 'node -e "require(\'fs\').writeFileSync(\'prepare\', \'\')"',
      postpack: 'node -e "require(\'fs\').writeFileSync(\'postpack\', \'\')"',
    },
  })

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })

  expect(await exists('test-publish-package.json-0.0.0.tgz')).toBeTruthy()
  expect(await exists('prepack')).toBeTruthy()
  expect(await exists('prepare')).toBeTruthy()
  expect(await exists('postpack')).toBeTruthy()
})

const modeIsExecutable = (mode: number) => (mode & 0o111) === 0o111

;(process.platform === 'win32' ? test.skip : test)('the mode of executable is changed', async () => {
  tempDir()

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: path.join(__dirname, '../fixtures/has-bin'),
    extraBinPaths: [],
    packDestination: process.cwd(),
  })

  await tar.x({ file: 'has-bin-0.0.0.tgz' })

  {
    const stat = fs.statSync(path.resolve('package/exec'))
    expect(modeIsExecutable(stat.mode)).toBeTruthy()
  }
  {
    const stat = fs.statSync(path.resolve('package/index.js'))
    expect(modeIsExecutable(stat.mode)).toBeFalsy()
  }
})
