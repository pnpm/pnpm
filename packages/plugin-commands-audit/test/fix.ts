import path from 'path'
import fixtures from '@pnpm/test-fixtures'
import { ProjectManifest } from '@pnpm/types'
import { audit } from '@pnpm/plugin-commands-audit'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import { response2, response3 } from '../response-mocks'

const f = fixtures(__dirname)

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, response2)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: true,
    registries: {
      default: registry,
    },
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = await loadJsonFile<ProjectManifest>(path.join(tmp, 'package.json'))
  expect(manifest.pnpm?.overrides?.['axios@<=0.18.0']).toBe('>=0.18.1')
  expect(manifest.pnpm?.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, response3)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: true,
    registries: {
      default: registry,
    },
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No fixes were made')
})
