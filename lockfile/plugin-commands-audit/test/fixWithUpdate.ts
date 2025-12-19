import { join } from 'path'
import { readFile } from 'fs/promises'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { type DepPath } from '@pnpm/types'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import nock from 'nock'
import { DEFAULT_OPTS, AUDIT_REGISTRY } from './index.js'

const f = fixtures(import.meta.dirname)

const registry = `http://localhost:${REGISTRY_MOCK_PORT}`

describe('audit fix with update', () => {
  test('depth-3 vulnerability is fixed by updating the vulnerable package', async () => {
    const tmp = f.prepare('update-linear-depth-3')

    const originalPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0' as DepPath
    const expectedPkgId = '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0' as DepPath

    const originalLockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(originalLockfile).toBeTruthy()
    expect(originalLockfile!.packages).toBeDefined()
    expect(originalLockfile!.packages![originalPkgId]).toBeDefined()
    expect(originalLockfile!.packages![expectedPkgId]).toBeUndefined()

    const mockResponse = await readFile(join(tmp, 'responses', 'depth-3-vulnerability.json'), 'utf-8')
    expect(mockResponse).toBeTruthy()

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(200, mockResponse)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: tmp,
      registries: { default: registry },
      rawConfig: { registry },
      userConfig: {},
      rootProjectManifestDir: tmp,
      auditLevel: 'moderate',
      fix: 'update',
      lockfileOnly: true,
    })

    expect(exitCode).toBe(0)
    expect(output).toMatch(/Packages were updated to fix vulnerabilities./)

    const lockfile = await readWantedLockfile(tmp, { ignoreIncompatible: true })
    expect(lockfile).toBeTruthy()
    expect(lockfile!.packages).toBeDefined()

    // The vulnerable dependency should be updated
    expect(lockfile!.packages![originalPkgId]).toBeUndefined()
    expect(lockfile!.packages![expectedPkgId]).toBeDefined()

    // All other packages should remain the same
    for (const pkgId of Object.keys(originalLockfile!.packages!)) {
      if (pkgId === originalPkgId) continue
      expect(lockfile!.packages![pkgId as DepPath]).toBeDefined()
    }
  })
})
