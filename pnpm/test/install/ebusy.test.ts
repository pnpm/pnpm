import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { execPnpm } from '../utils'

const f = fixtures(__dirname)

test('there should be no EBUSY error on Windows', async () => {
  const tmp = tempDir()
  f.copy('platformatic', tmp)
  await execPnpm(['install', '--ignore-scripts'])
})
