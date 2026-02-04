import { type PackResult } from '../src/pack.js'
import { type FailedToPublishError, createFailedToPublishError } from '../src/FailedToPublishError.js'

const pack = (): PackResult => ({
  contents: ['index.js', 'bin.js'],
  publishedManifest: {
    name: 'example-pack',
    version: '0.1.2',
  },
  tarballPath: 'example-pack.tgz',
})

describe('createFailedToPublishError', () => {
  test('without details', async () => {
    expect(await createFailedToPublishError(pack(), {
      status: 401,
      statusText: 'Unauthorized',
      text: () => '',
    })).toMatchObject({
      code: 'ERR_PNPM_FAILED_TO_PUBLISH',
      message: 'Failed to publish package example-pack@0.1.2 (status 401 Unauthorized)',
      status: 401,
      statusText: 'Unauthorized',
      text: '',
      pack: pack(),
    } as Partial<FailedToPublishError<PackResult>>)
  })

  test('failed to get details text', async () => {
    expect(await createFailedToPublishError(pack(), {
      status: 401,
      statusText: 'Unauthorized',
      text () {
        throw new Error('No details')
      },
    })).toMatchObject({
      code: 'ERR_PNPM_FAILED_TO_PUBLISH',
      message: 'Failed to publish package example-pack@0.1.2 (status 401 Unauthorized)',
      status: 401,
      statusText: 'Unauthorized',
      text: '',
      pack: pack(),
    } as Partial<FailedToPublishError<PackResult>>)
  })

  test('with single-line details', async () => {
    const text = 'Failed to authenticate'
    expect(await createFailedToPublishError(pack(), {
      status: 401,
      statusText: 'Unauthorized',
      text: () => text,
    })).toMatchObject({
      code: 'ERR_PNPM_FAILED_TO_PUBLISH',
      message: 'Failed to publish package example-pack@0.1.2 (status 401 Unauthorized): Failed to authenticate',
      status: 401,
      statusText: 'Unauthorized',
      text,
      pack: pack(),
    } as Partial<FailedToPublishError<PackResult>>)
  })

  test('with multi-line details', async () => {
    const text = [
      'Failed to authenticate',
      'No token provided',
    ].join('\n')
    expect(await createFailedToPublishError(pack(), {
      status: 401,
      statusText: 'Unauthorized',
      text: () => text,
    })).toMatchObject({
      code: 'ERR_PNPM_FAILED_TO_PUBLISH',
      message: [
        'Failed to publish package example-pack@0.1.2 (status 401 Unauthorized)',
        'Details:',
        '    Failed to authenticate',
        '    No token provided',
        '',
      ].join('\n'),
      status: 401,
      statusText: 'Unauthorized',
      text,
      pack: pack(),
    } as Partial<FailedToPublishError<PackResult>>)
  })

  test('with an empty statusText', async () => {
    expect(await createFailedToPublishError(pack(), {
      status: 499,
      statusText: '',
      text: () => '',
    })).toMatchObject({
      code: 'ERR_PNPM_FAILED_TO_PUBLISH',
      message: 'Failed to publish package example-pack@0.1.2 (status 499)',
      status: 499,
      statusText: '',
      text: '',
      pack: pack(),
    } as Partial<FailedToPublishError<PackResult>>)
  })
})
