import prepare, { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import { execPnpm } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

const CREDENTIALS = [
  '--//localhost:4873/:username=username',
  `--//localhost:4873/:_password=${Buffer.from('password').toString('base64')}`,
  '--//localhost:4873/:email=foo@bar.net',
]

test('publish: package with package.json', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execPnpm('publish', ...CREDENTIALS)
})

test('publish: package with package.yaml', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await execPnpm('publish', ...CREDENTIALS)

  t.ok(await exists('package.yaml'))
  t.notOk(await exists('package.json'))
})

test('publish: package with package.json5', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await execPnpm('publish', ...CREDENTIALS)

  t.ok(await exists('package.json5'))
  t.notOk(await exists('package.json'))
})

test('publish: package with package.json5 running publish from different folder', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.1',
  }, { manifestFormat: 'JSON5' })

  process.chdir('..')

  await execPnpm('publish', 'project', ...CREDENTIALS)

  t.ok(await exists('project/package.json5'))
  t.notOk(await exists('project/package.json'))
})

test('pack packages with workspace LICENSE if no own LICENSE is present', async (t: tape.Test) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
    {
      name: 'target',
      version: '1.0.0',
    },
  ], { manifestFormat: 'YAML' })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-2/LICENSE', 'project-2 license', 'utf8')

  process.chdir('project-1')
  await execPnpm('pack')

  process.chdir('../project-2')
  await execPnpm('pack')

  process.chdir('../target')

  await execPnpm('add', '../project-1/project-1-1.0.0.tgz', '../project-2/project-2-1.0.0.tgz')

  t.equal(await fs.readFile('node_modules/project-1/LICENSE', 'utf8'), 'workspace license')
  t.equal(await fs.readFile('node_modules/project-2/LICENSE', 'utf8'), 'project-2 license')

  process.chdir('..')
  t.notOk(await exists('project-1/LICENSE'))
  t.ok(await exists('project-2/LICENSE'))
})

test('publish packages with workspace LICENSE if no own LICENSE is present', async (t: tape.Test) => {
  preparePackages(t, [
    {
      name: 'project-100',
      version: '1.0.0',
    },
    {
      name: 'project-200',
      version: '1.0.0',
    },
    {
      name: 'target',
      version: '1.0.0',
    },
  ], { manifestFormat: 'YAML' })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-200/LICENSE', 'project-200 license', 'utf8')

  process.chdir('project-100')
  await execPnpm('publish')

  process.chdir('../project-200')
  await execPnpm('publish')

  process.chdir('../target')

  await execPnpm('add', 'project-100', 'project-200', '--no-link-workspace-packages')

  t.equal(await fs.readFile('node_modules/project-100/LICENSE', 'utf8'), 'workspace license')
  t.equal(await fs.readFile('node_modules/project-200/LICENSE', 'utf8'), 'project-200 license')

  process.chdir('..')
  t.notOk(await exists('project-100/LICENSE'))
  t.ok(await exists('project-200/LICENSE'))
})
