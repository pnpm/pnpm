import path from 'path'
import { copyFixture } from '@pnpm/test-fixtures'
import { ProjectManifest } from '@pnpm/types'
import { audit } from '@pnpm/plugin-commands-audit'
import loadJsonFile from 'load-json-file'
import tempy from 'tempy'

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = tempy.directory()
  await copyFixture('has-vulnerabilities', tmp, __dirname)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = await loadJsonFile<ProjectManifest>(path.join(tmp, 'package.json'))
  expect(manifest.pnpm?.overrides?.['axios@<0.18.1']).toBe('>=0.18.1')
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = tempy.directory()
  await copyFixture('fixture', tmp, __dirname)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No fixes were made')
})
