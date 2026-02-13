import fs from 'fs'
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { prepare } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { DEFAULT_OPTS } from './utils/index.js'

const original = await import('@pnpm/logger')
const warn = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => {
  const logger = {
    ...original.logger,
    warn,
  }
  return {
    ...original,
    globalWarn: jest.fn(),
    logger: Object.assign(() => logger, logger),
  }
})
const { globalWarn } = await import('@pnpm/logger')
const { eject } = await import('@pnpm/plugin-commands-deploy')

beforeEach(async () => {
  jest.mocked(globalWarn).mockClear()
})

afterEach(() => {
  jest.restoreAllMocks()
})

test('eject basic dependencies', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    dependenciesMeta: {
      'is-positive': { ejected: true },
    },
  })

  await eject.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['dist'])

  const project = assertProject(path.resolve('dist'))
  project.has('is-positive')
  project.hasNot('is-negative')

  expect(fs.existsSync('dist/package.json')).toBeTruthy()
  expect(fs.existsSync('dist/node_modules/.modules.yaml')).toBeTruthy()
})

test('eject with no ejected dependencies should throw', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await expect(
    eject.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['dist'])
  ).rejects.toThrow('No dependencies marked with "ejected: true" found in dependenciesMeta')
})

test('eject to non-empty directory without --force should throw', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'is-positive': { ejected: true },
    },
  })

  fs.mkdirSync('dist', { recursive: true })
  fs.writeFileSync('dist/file.txt', 'test', 'utf8')

  const distFullPath = path.resolve('dist')

  await expect(
    eject.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['dist'])
  ).rejects.toThrow(`Target directory ${distFullPath} is not empty`)
})

test('eject with --force should overwrite non-empty directory', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    dependenciesMeta: {
      'is-positive': { ejected: true },
    },
  })

  fs.mkdirSync('dist', { recursive: true })
  fs.writeFileSync('dist/file.txt', 'test', 'utf8')

  const distFullPath = path.resolve('dist')

  await eject.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    force: true,
  }, ['dist'])

  expect(warn).toHaveBeenCalledWith({
    message: expect.stringMatching(/^using --force, deleting target directory/),
    prefix: distFullPath,
  })

  expect(fs.existsSync('dist/file.txt')).toBeFalsy()

  const project = assertProject(path.resolve('dist'))
  project.has('is-positive')

  warn.mockRestore()
})

test('eject respects --prod flag', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
    dependenciesMeta: {
      'is-positive': { ejected: true },
      'is-negative': { ejected: true },
    },
  })

  await eject.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    production: true,
    dev: false,
  }, ['dist'])

  const project = assertProject(path.resolve('dist'))

  // Include only normal dependencies and not devDependencies
  project.has('is-positive')
  project.hasNot('is-negative')
})

test('eject with multiple dependencies', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
      'is-odd': '1.0.0',
    },
    dependenciesMeta: {
      'is-positive': { ejected: true },
      'is-negative': { ejected: true },
    },
  })

  await eject.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['dist'])

  const project = assertProject(path.resolve('dist'))

  project.has('is-positive')
  project.has('is-negative')
  project.hasNot('is-odd')
})

test('eject without parameters should throw', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
  })

  await expect(
    eject.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow('This command requires one parameter: the target directory')
})

test('eject with multiple parameters should throw', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
  })

  await expect(
    eject.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['dist', 'another/path']) // Multiple params
  ).rejects.toThrow('This command requires one parameter: the target directory')
})

test('eject from project with optionalDependencies', async () => {
  prepare({
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      'is-negative': '1.0.0',
    },
    dependenciesMeta: {
      'is-positive': { ejected: true },
      'is-negative': { ejected: true },
    },
  })

  await eject.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['dist'])

  const project = assertProject(path.resolve('dist'))

  project.has('is-positive')
  project.has('is-negative')
})
