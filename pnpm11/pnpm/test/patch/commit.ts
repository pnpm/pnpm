import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test('patch and commit with a dash-prefixed relative edit dir', () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['.'] })
  const cliOptions = {
    env: {
      pnpm_config_cache_dir: path.resolve('cache'),
      pnpm_config_store_dir: path.resolve('store'),
    },
    expectSuccess: true,
  }
  execPnpmSync(['install'], cliOptions)

  const editDir = '-custom-edit-dir'
  execPnpmSync(['patch', 'is-positive@1.0.0', `--edit-dir=./${editDir}`], cliOptions)

  fs.appendFileSync(path.join(editDir, 'index.js'), '// test patching', 'utf8')

  execPnpmSync(['patch-commit', '--', editDir], cliOptions)

  const patchContent = fs.readFileSync('patches/is-positive@1.0.0.patch', 'utf8')
  expect(patchContent).toContain('diff --git a/index.js b/index.js')
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// test patching')
})
