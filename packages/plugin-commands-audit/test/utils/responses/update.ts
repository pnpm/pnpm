import fs from 'fs'
import path from 'path'
import audit from '@pnpm/audit'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import fixtures from '@pnpm/test-fixtures'

const f = fixtures(__dirname)

async function writeResponse (lockfileDir: string, filename: string, opts: {
  production?: boolean
  dev?: boolean
  optional?: boolean
}) {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const auditReport = await audit(lockfile!, {
    agentOptions: {},
    include,
    registry: 'https://registry.npmjs.org/',
  })
  fs.writeFileSync(path.join(__dirname, filename), JSON.stringify(auditReport, null, 2))
}

// eslint-disable-next-line
; (async () => {
  await writeResponse(f.find('has-vulnerabilities'), 'dev-vulnerabilities-only-response.json', {
    dev: true,
    production: false,
  })
  await writeResponse(f.find('has-vulnerabilities'), 'all-vulnerabilities-response.json', {})
  await writeResponse(f.find('has-outdated-deps'), 'no-vulnerabilities-response.json', {})
})()
