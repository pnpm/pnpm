import fs from 'fs'
import path from 'path'
import { pack } from '@pnpm/plugin-commands-publishing'
import { prepare, tempDir } from '@pnpm/prepare'
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

test('pack a package without package name', async () => {
  prepare({
    name: undefined,
    version: '0.0.0',
  })

  await expect(pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })).rejects.toThrow('Package name is not defined in the package.json.')
})

test('pack a package without package version', async () => {
  prepare({
    name: 'test-publish-package-no-version',
    version: undefined,
  })

  await expect(pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
  })).rejects.toThrow('Package version is not defined in the package.json.')
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
    const stat = fs.statSync(path.resolve('package/other-exec'))
    expect(modeIsExecutable(stat.mode)).toBeTruthy()
  }
  {
    const stat = fs.statSync(path.resolve('package/index.js'))
    expect(modeIsExecutable(stat.mode)).toBeFalsy()
  }
})

test('pack: should embed readme', async () => {
  tempDir()

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: path.join(__dirname, '../fixtures/readme'),
    extraBinPaths: [],
    packDestination: process.cwd(),
    embedReadme: true,
  })

  await tar.x({ file: 'readme-0.0.0.tgz' })

  const pkg = await import(path.resolve('package/package.json'))

  expect(pkg.readme).toBeTruthy()
})

test('pack: should not embed readme', async () => {
  tempDir()

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: path.join(__dirname, '../fixtures/readme'),
    extraBinPaths: [],
    packDestination: process.cwd(),
    embedReadme: false,
  })

  await tar.x({ file: 'readme-0.0.0.tgz' })

  const pkg = await import(path.resolve('package/package.json'))

  expect(pkg.readme).toBeFalsy()
})

test('pack: remove publishConfig', async () => {
  prepare({
    name: 'remove-publish-config',
    version: '0.0.0',
    main: 'index.d.js',
    publishConfig: {
      types: 'index.d.ts',
      main: 'index.js',
    },
  })

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
    packDestination: process.cwd(),
    embedReadme: false,
  })

  await tar.x({ file: 'remove-publish-config-0.0.0.tgz' })

  expect((await import(path.resolve('package/package.json'))).default).toStrictEqual({
    name: 'remove-publish-config',
    version: '0.0.0',
    main: 'index.js',
    types: 'index.d.ts',
  })
})

test('pack should read from the correct node_modules when publishing from a custom directory', async () => {
  prepare({
    name: 'custom-publish-dir',
    version: '0.0.0',
    publishConfig: {
      directory: 'dist',
    },
    dependencies: {
      local: 'workspace:*',
    },
  })

  fs.mkdirSync('dist')
  fs.copyFileSync('package.json', 'dist/package.json')
  fs.mkdirSync('node_modules/local', { recursive: true })
  fs.writeFileSync('node_modules/local/package.json', JSON.stringify({ name: 'local', version: '1.0.0' }), 'utf8')

  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
    packDestination: process.cwd(),
  })

  await tar.x({ file: 'custom-publish-dir-0.0.0.tgz' })

  expect((await import(path.resolve('package/package.json'))).default).toStrictEqual({
    name: 'custom-publish-dir',
    version: '0.0.0',
    dependencies: {
      local: '1.0.0',
    },
    publishConfig: {
      directory: 'dist',
    },
  })
})

test('pack to custom destination directory', async () => {
  prepare({
    name: 'custom-dest',
    version: '0.0.0',
  })

  const output = await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
    packDestination: path.resolve('custom-dest'),
    embedReadme: false,
  })

  expect(output).toBe(path.resolve('custom-dest/custom-dest-0.0.0.tgz'))
})
