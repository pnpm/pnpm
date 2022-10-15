import { buildSequence } from '../lib/buildSequence'

test('buildSequence() test 1', () => {
  const chunks = buildSequence({
    '/a/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/b/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/c/1.0.0': {
      children: {},
      requiresBuild: true,
    },
  }, ['/a/1.0.0', '/b/1.0.0'])
  expect(chunks).toStrictEqual([
    ['/c/1.0.0'],
    ['/a/1.0.0', '/b/1.0.0'],
  ])
})

test('buildSequence() test 2', () => {
  const chunks = buildSequence({
    '/a/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/b/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
    },
    '/c/1.0.0': {
      children: {},
      requiresBuild: true,
    },
  }, ['/a/1.0.0', '/b/1.0.0'])
  expect(chunks).toStrictEqual([
    ['/c/1.0.0'],
    ['/a/1.0.0'],
  ])
})

test('buildSequence() test 3', () => {
  const chunks = buildSequence({
    '/a/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/b/1.0.0': {
      children: {
        d: '/d/1.0.0',
      },
    },
    '/c/1.0.0': {
      children: {},
      requiresBuild: true,
    },
    '/d/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
  }, ['/a/1.0.0', '/b/1.0.0'])
  expect(chunks).toStrictEqual([
    ['/c/1.0.0'],
    ['/a/1.0.0', '/d/1.0.0'],
    ['/b/1.0.0'],
  ])
})
