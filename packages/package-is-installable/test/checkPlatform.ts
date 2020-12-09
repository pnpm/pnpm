import checkPlatform from '../lib/checkPlatform'

const packageId = 'registry.npmjs.org/foo/1.0.0'

test('target cpu wrong', () => {
  const target = {
    cpu: 'enten-cpu',
    os: 'any',
  }
  const err = checkPlatform(packageId, target)
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('os wrong', () => {
  const target = {
    cpu: 'any',
    os: 'enten-os',
  }
  const err = checkPlatform(packageId, target)
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('nothing wrong', () => {
  const target = {
    cpu: 'any',
    os: 'any',
  }
  expect(checkPlatform(packageId, target)).toBeFalsy()
})

test('only target cpu wrong', () => {
  const err = checkPlatform(packageId, { cpu: 'enten-cpu', os: 'any' })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('only os wrong', () => {
  const err = checkPlatform(packageId, { cpu: 'any', os: 'enten-os' })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('everything wrong w/arrays', () => {
  const err = checkPlatform(packageId, { cpu: ['enten-cpu'], os: ['enten-os'] })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('os wrong (negation)', () => {
  const err = checkPlatform(packageId, { cpu: 'any', os: `!${process.platform}` })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('nothing wrong (negation)', () => {
  expect(checkPlatform(packageId, { cpu: '!enten-cpu', os: '!enten-os' })).toBe(null)
})
