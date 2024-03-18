import fs from 'node:fs'
import path from 'node:path'
import { audit } from '@pnpm/audit'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(__dirname)

async function writeResponse(
  lockfileDir: string,
  filename: string,
  opts: {
    production?: boolean
    dev?: boolean
    optional?: boolean
  }
): Promise<void> {
  const lockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: true,
  })

  if (lockfile === null) {
    return
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const auditReport = await audit(lockfile, (s) => {
    return s
  }, {
    include,
    registry: 'https://registry.npmjs.org/',
    lockfileDir: '',
  })
  fs.writeFileSync(
    path.join(__dirname, filename),
    JSON.stringify(auditReport, null, 2)
  )
}

;(async () => {
  await writeResponse(
    f.find('has-vulnerabilities'),
    'dev-vulnerabilities-only-response.json',
    {
      dev: true,
      production: false,
    }
  )
  await writeResponse(
    f.find('has-vulnerabilities'),
    'all-vulnerabilities-response.json',
    {}
  )
  await writeResponse(
    f.find('has-outdated-deps'),
    'no-vulnerabilities-response.json',
    {}
  )
})()
