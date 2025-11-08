import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import writeYamlFile from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils/index.js'

const debug = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => {
  return {
    logger: () => ({ debug }),
    globalInfo: jest.fn(),
    globalWarn: jest.fn(),
    streamParser: jest.fn(),
  }
})

const { filterPackagesFromDir } = await import('@pnpm/workspace.filter-packages-from-dir')
const { exec } = await import('@pnpm/plugin-commands-script-runners')

afterEach(() => {
  jest.mocked(debug).mockClear()
})

test('pnpm exec --recursive --no-reporter-hide-prefix prints prefixes', async () => {
  preparePackages([
    {
      location: 'packages/foo',
      package: { name: 'foo' },
    },
    {
      location: 'packages/bar',
      package: { name: 'bar' },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const { selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

  const scriptFile = path.resolve('script.js')
  fs.writeFileSync(scriptFile, `
    console.log('hello from stdout')
    console.error('hello from stderr')
    console.log('name is ' + require(require('path').resolve('package.json')).name)
  `)

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    bail: true,
    reporterHidePrefix: false,
    selectedProjectsGraph,
  }, [process.execPath, scriptFile])

  for (const name of ['foo', 'bar']) {
    const loggerOpts = {
      wd: path.resolve('packages', name),
      depPath: name,
      stage: '(exec)',
    }
    expect(debug).toHaveBeenCalledWith({
      ...loggerOpts,
      line: 'hello from stdout',
      stdio: 'stdout',
    })
    expect(debug).toHaveBeenCalledWith({
      ...loggerOpts,
      line: 'hello from stderr',
      stdio: 'stderr',
    })
    expect(debug).toHaveBeenCalledWith({
      ...loggerOpts,
      line: `name is ${name}`,
      stdio: 'stdout',
    })
    expect(debug).toHaveBeenCalledWith({
      ...loggerOpts,
      optional: false,
      exitCode: 0,
    })
  }
})

test('pnpm exec --recursive --reporter-hide-prefix does not print prefixes', async () => {
  preparePackages([
    {
      location: 'packages/foo',
      package: { name: 'foo' },
    },
    {
      location: 'packages/bar',
      package: { name: 'bar' },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const { selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

  const scriptFile = path.resolve('script.js')
  fs.writeFileSync(scriptFile, `
    console.log('hello from stdout')
    console.error('hello from stderr')
    console.log('name is ' + require(require('path').resolve('package.json')).name)
  `)

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    bail: true,
    reporterHidePrefix: true,
    selectedProjectsGraph,
  }, [process.execPath, scriptFile])

  expect(debug).not.toHaveBeenCalled()
})

test('pnpm exec --recursive does not print prefixes by default', async () => {
  preparePackages([
    {
      location: 'packages/foo',
      package: { name: 'foo' },
    },
    {
      location: 'packages/bar',
      package: { name: 'bar' },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const { selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

  const scriptFile = path.resolve('script.js')
  fs.writeFileSync(scriptFile, `
    console.log('hello from stdout')
    console.error('hello from stderr')
    console.log('name is ' + require(require('path').resolve('package.json')).name)
  `)

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    bail: true,
    selectedProjectsGraph,
  }, [process.execPath, scriptFile])

  expect(debug).not.toHaveBeenCalled()
})
