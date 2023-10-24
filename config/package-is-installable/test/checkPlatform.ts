import { checkPlatform } from '../lib/checkPlatform'

const packageId = 'registry.npmjs.org/foo/1.0.0'

jest.mock('detect-libc', () => {
  const original = jest.requireActual('detect-libc')
  return {
    ...original,
    familySync: () => 'musl',
  }
})

test('target cpu wrong', () => {
  const target = {
    cpu: 'enten-cpu',
    os: 'any',
    libc: 'any',
  }
  const err = checkPlatform(packageId, target)
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('os wrong', () => {
  const target = {
    cpu: 'any',
    os: 'enten-os',
    libc: 'any',
  }
  const err = checkPlatform(packageId, target)
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('libc wrong', () => {
  const target = {
    cpu: 'any',
    os: 'any',
    libc: 'enten-libc',
  }
  const err = checkPlatform(packageId, target)
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('nothing wrong', () => {
  const target = {
    cpu: 'any',
    os: 'any',
    libc: 'any',
  }
  expect(checkPlatform(packageId, target)).toBeFalsy()
})

test('only target cpu wrong', () => {
  const err = checkPlatform(packageId, { cpu: 'enten-cpu', os: 'any', libc: 'any' })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('only os wrong', () => {
  const err = checkPlatform(packageId, { cpu: 'any', os: 'enten-os', libc: 'any' })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('everything wrong w/arrays', () => {
  const err = checkPlatform(packageId, { cpu: ['enten-cpu'], os: ['enten-os'], libc: ['enten-libc'] })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('os wrong (negation)', () => {
  const err = checkPlatform(packageId, { cpu: 'any', os: `!${process.platform}`, libc: 'any' })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('nothing wrong (negation)', () => {
  expect(checkPlatform(packageId, { cpu: '!enten-cpu', os: '!enten-os', libc: '!enten-libc' })).toBe(null)
})

test('override OS', () => {
  expect(checkPlatform(packageId, { cpu: 'any', os: 'win32', libc: 'any' }, {
    os: ['win32'],
    cpu: ['current'],
    libc: ['current'],
  })).toBe(null)
})

test('accept another CPU', () => {
  expect(checkPlatform(packageId, { cpu: 'x64', os: 'any', libc: 'any' }, {
    os: ['current'],
    cpu: ['current', 'x64'],
    libc: ['current'],
  })).toBe(null)
})

test('fail when CPU is different', () => {
  const err = checkPlatform(packageId, { cpu: 'x64', os: 'any', libc: 'any' }, {
    os: ['current'],
    cpu: ['arm64'],
    libc: ['current'],
  })
  expect(err).toBeTruthy()
  expect(err?.code).toBe('ERR_PNPM_UNSUPPORTED_PLATFORM')
})

test('override libc', () => {
  expect(checkPlatform(packageId, { cpu: 'any', os: 'any', libc: 'glibc' }, {
    os: ['current'],
    cpu: ['current'],
    libc: ['glibc'],
  })).toBe(null)
})

test('accept another libc', () => {
  expect(checkPlatform(packageId, { cpu: 'any', os: 'any', libc: 'glibc' }, {
    os: ['current'],
    cpu: ['current'],
    libc: ['current', 'glibc'],
  })).toBe(null)
})
