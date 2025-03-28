import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils'

test('patch from configuration dependency is applied', async () => {
  prepare()
  writeYamlFile('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/has-patch-for-foo': `1.0.0+${getIntegrity('@pnpm.e2e/has-patch-for-foo', '1.0.0')}`,
    },
    patchedDependencies: {
      '@pnpm.e2e/foo@100.0.0': 'node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo/@pnpm.e2e__foo@100.0.0.patch',
    },
  })

  await execPnpm(['add', '@pnpm.e2e/foo@100.0.0'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/foo/index.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile', async () => {
  prepare({})
  writeYamlFile('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/build-allow-list': `1.0.0+${getIntegrity('@pnpm.e2e/build-allow-list', '1.0.0')}`,
    },
    onlyBuiltDependenciesFile: 'node_modules/.pnpm-config/@pnpm.e2e/build-allow-list/list.json',
  })

  await execPnpm(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await execPnpm(['install'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})

test('selectively allow scripts in some dependencies by onlyBuiltDependenciesFile and onlyBuiltDependencies', async () => {
  prepare()
  writeYamlFile('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/build-allow-list': `1.0.0+${getIntegrity('@pnpm.e2e/build-allow-list', '1.0.0')}`,
    },
    onlyBuiltDependenciesFile: 'node_modules/.pnpm-config/@pnpm.e2e/build-allow-list/list.json',
    onlyBuiltDependencies: ['@pnpm.e2e/pre-and-postinstall-scripts-example'],
  })

  await execPnpm(['add', '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0', '@pnpm.e2e/install-script-example'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()

  rimraf('node_modules')

  await execPnpm(['install'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/install-script-example/generated-by-install.js')).toBeTruthy()
})
