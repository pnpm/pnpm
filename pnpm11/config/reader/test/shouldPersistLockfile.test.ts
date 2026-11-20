import { describe, expect, test } from '@jest/globals'
import { shouldPersistLockfile } from '@pnpm/config.reader'

describe('shouldPersistLockfile', () => {
  test('devEngines.packageManager persists unless onFail is ignore', () => {
    expect(shouldPersistLockfile({ version: '9.3.0', fromDevEngines: true })).toBe(true)
    expect(shouldPersistLockfile({ version: '11.0.0', fromDevEngines: true })).toBe(true)
    expect(shouldPersistLockfile({ version: '12.0.0', fromDevEngines: true })).toBe(true)
    expect(shouldPersistLockfile({ version: '>=9.0.0', fromDevEngines: true })).toBe(true)
    expect(shouldPersistLockfile({ version: '>=9.0.0', fromDevEngines: true, onFail: 'ignore' })).toBe(false)
  })

  test('packageManager field with pnpm v11 or older does not persist', () => {
    expect(shouldPersistLockfile({ version: '9.3.0' })).toBe(false)
    expect(shouldPersistLockfile({ version: '10.0.0' })).toBe(false)
    expect(shouldPersistLockfile({ version: '11.0.0' })).toBe(false)
    expect(shouldPersistLockfile({ version: '11.0.0-rc.1' })).toBe(false)
  })

  test('packageManager field with pnpm v12 or newer persists', () => {
    expect(shouldPersistLockfile({ version: '12.0.0' })).toBe(true)
    expect(shouldPersistLockfile({ version: '12.5.3' })).toBe(true)
    expect(shouldPersistLockfile({ version: '13.0.0' })).toBe(true)
    expect(shouldPersistLockfile({ version: '100.0.0' })).toBe(true)
    expect(shouldPersistLockfile({ version: '12.0.0', onFail: 'ignore' })).toBe(false)
  })

  test('missing or invalid version does not persist', () => {
    expect(shouldPersistLockfile({ version: undefined })).toBe(false)
    expect(shouldPersistLockfile({ version: 'not-a-version' })).toBe(false)
    // Ranges are not valid for the legacy packageManager field. Its parser
    // rejects them, but the persistence gate still treats them as non-pinning.
    expect(shouldPersistLockfile({ version: '^12.0.0' })).toBe(false)
  })
})
