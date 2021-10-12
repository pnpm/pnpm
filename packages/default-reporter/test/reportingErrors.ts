import path from 'path'
import { toOutput$ } from '@pnpm/default-reporter'
import PnpmError from '@pnpm/error'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import { map, take } from 'rxjs/operators'
import chalk from 'chalk'
import loadJsonFile from 'load-json-file'
import normalizeNewline from 'normalize-newline'
import StackTracey from 'stacktracey'

const formatErrorCode = (code: string) => chalk.bgRed.black(`\u2009${code}\u2009`)
const formatError = (code: string, message: string) => {
  return `${formatErrorCode(code)} ${chalk.red(message)}`
}
const ERROR_PAD = ''

test('prints generic error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  logger.error(err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERROR', 'some error')}
${ERROR_PAD}${(new StackTracey(err.stack).pretty as string).split('\n').join(`\n${ERROR_PAD}`)}`)
    },
  })
})

test('prints generic error when recursive install fails', (done) => {
  const output$ = toOutput$({
    context: { argv: ['recursive'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  err['prefix'] = '/home/src/'
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`/home/src/:
${formatError('ERROR', 'some error')}
${ERROR_PAD}
${ERROR_PAD}${(new StackTracey(err.stack).pretty as string).split('\n').join(`\n${ERROR_PAD}`)}`)
    },
  })
})

test('prints no matching version error when many dist-tags exist', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_NO_MATCHING_VERSION', 'No matching version found for pnpm@1000.0.0')}
${ERROR_PAD}
${ERROR_PAD}The latest release of pnpm is "2.4.0".
${ERROR_PAD}
${ERROR_PAD}Other releases are:
${ERROR_PAD}  * stable: 2.2.2
${ERROR_PAD}  * next: 2.4.0
${ERROR_PAD}  * latest-1: 1.43.1
${ERROR_PAD}
${ERROR_PAD}If you need the full list of all 281 published versions run "$ pnpm view pnpm versions".`)
    },
  })

  const err = new PnpmError('NO_MATCHING_VERSION', 'No matching version found for pnpm@1000.0.0')
  err['packageMeta'] = loadJsonFile.sync(path.join(__dirname, 'pnpm-meta.json'))
  logger.error(err, err)
})

test('prints no matching version error when only the latest dist-tag exists', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_NO_MATCHING_VERSION', 'No matching version found for is-positive@1000.0.0')}
${ERROR_PAD}
${ERROR_PAD}The latest release of is-positive is "3.1.0".
${ERROR_PAD}
${ERROR_PAD}If you need the full list of all 4 published versions run "$ pnpm view is-positive versions".`)
    },
  })

  const err = new PnpmError('NO_MATCHING_VERSION', 'No matching version found for is-positive@1000.0.0')
  err['packageMeta'] = loadJsonFile.sync(path.join(__dirname, 'is-positive-meta.json'))
  logger.error(err, err)
})

test('prints suggestions when an internet-connection related error happens', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_BAD_TARBALL_SIZE', 'Actual size (99) of tarball (https://foo) did not match the one specified in \'Content-Length\' header (100)')}
${ERROR_PAD}
${ERROR_PAD}Seems like you have internet connection issues.
${ERROR_PAD}Try running the same command again.
${ERROR_PAD}If that doesn't help, try one of the following:
${ERROR_PAD}
${ERROR_PAD}- Set a bigger value for the \`fetch-retries\` config.
${ERROR_PAD}    To check the current value of \`fetch-retries\`, run \`pnpm get fetch-retries\`.
${ERROR_PAD}    To set a new value, run \`pnpm set fetch-retries <number>\`.
${ERROR_PAD}
${ERROR_PAD}- Set \`network-concurrency\` to 1.
${ERROR_PAD}    This change will slow down installation times, so it is recommended to
${ERROR_PAD}    delete the config once the internet connection is good again: \`pnpm config delete network-concurrency\`
${ERROR_PAD}
${ERROR_PAD}NOTE: You may also override configs via flags.
${ERROR_PAD}For instance, \`pnpm install --fetch-retries 5 --network-concurrency 1\``)
    },
  })

  const err = new PnpmError('BAD_TARBALL_SIZE', 'Actual size (99) of tarball (https://foo) did not match the one specified in \'Content-Length\' header (100)')
  err['expectedSize'] = 100
  err['receivedSize'] = 99
  logger.error(err, err)
})

test('prints test error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['run', 'test'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ELIFECYCLE', 'Test failed. See above for more details.')}`)
    },
  })

  const err = new Error('Tests failed')
  err['stage'] = 'test'
  err['code'] = 'ELIFECYCLE'
  logger.error(err, err)
})

test('prints command error with exit code', (done) => {
  const output$ = toOutput$({
    context: { argv: ['run', 'lint'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ELIFECYCLE', 'Command failed with exit code 100.')}`)
    },
  })

  const err = new Error('Command failed')
  err['errno'] = 100
  err['stage'] = 'lint'
  err['code'] = 'ELIFECYCLE'
  logger.error(err, err)
})

test('prints command error without exit code', (done) => {
  const output$ = toOutput$({
    context: { argv: ['run', 'lint'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ELIFECYCLE', 'Command failed.')}`)
    },
  })

  const err = new Error('Command failed')
  err['stage'] = 'lint'
  err['code'] = 'ELIFECYCLE'
  logger.error(err, err)
})

test('prints unsupported pnpm version error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_UNSUPPORTED_ENGINE', 'Unsupported environment (bad pnpm and/or Node.js version)')}
${ERROR_PAD}
${ERROR_PAD}Your pnpm version is incompatible with "/home/zoltan/project".
${ERROR_PAD}
${ERROR_PAD}Expected version: 2
${ERROR_PAD}Got: 3.0.0
${ERROR_PAD}
${ERROR_PAD}This is happening because the package's manifest has an engines.pnpm field specified.
${ERROR_PAD}To fix this issue, install the required pnpm version globally.
${ERROR_PAD}
${ERROR_PAD}To install the latest version of pnpm, run "pnpm i -g pnpm".
${ERROR_PAD}To check your pnpm version, run "pnpm -v".`)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { pnpm: '2' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints unsupported Node version error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_UNSUPPORTED_ENGINE', 'Unsupported environment (bad pnpm and/or Node.js version)')}
${ERROR_PAD}
${ERROR_PAD}Your Node version is incompatible with "/home/zoltan/project".
${ERROR_PAD}
${ERROR_PAD}Expected version: >=12
${ERROR_PAD}Got: 10.0.0
${ERROR_PAD}
${ERROR_PAD}This is happening because the package's manifest has an engines.node field specified.
${ERROR_PAD}To fix this issue, install the required Node version.`)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { node: '>=12' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints unsupported pnpm and Node versions error', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_UNSUPPORTED_ENGINE', 'Unsupported environment (bad pnpm and/or Node.js version)')}
${ERROR_PAD}
${ERROR_PAD}Your pnpm version is incompatible with "/home/zoltan/project".
${ERROR_PAD}
${ERROR_PAD}Expected version: 2
${ERROR_PAD}Got: 3.0.0
${ERROR_PAD}
${ERROR_PAD}This is happening because the package's manifest has an engines.pnpm field specified.
${ERROR_PAD}To fix this issue, install the required pnpm version globally.
${ERROR_PAD}
${ERROR_PAD}To install the latest version of pnpm, run "pnpm i -g pnpm".
${ERROR_PAD}To check your pnpm version, run "pnpm -v".` + '\n\n' + `\
${ERROR_PAD}Your Node version is incompatible with "/home/zoltan/project".
${ERROR_PAD}
${ERROR_PAD}Expected version: >=12
${ERROR_PAD}Got: 10.0.0
${ERROR_PAD}
${ERROR_PAD}This is happening because the package's manifest has an engines.node field specified.
${ERROR_PAD}To fix this issue, install the required Node version.`)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { pnpm: '2', node: '>=12' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints error even if the error object not passed in through the message object', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error')
  logger.error(err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(formatError('ERR_PNPM_SOME_ERROR', 'some error'))
    },
  })
})

test('prints error without packages stacktrace when pkgsStack is empty', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error')
  err.pkgsStack = []
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(formatError('ERR_PNPM_SOME_ERROR', 'some error'))
    },
  })
})

test('prints error with packages stacktrace - depth 1 and hint', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error', { hint: 'hint' })
  err.pkgsStack = [
    {
      id: 'registry.npmjs.org/foo/1.0.0',
      name: 'foo',
      version: '1.0.0',
    },
  ]
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_SOME_ERROR', 'some error')}
${ERROR_PAD}
${ERROR_PAD}This error happened while installing the dependencies of foo@1.0.0
${ERROR_PAD}hint`)
    },
  })
})

test('prints error with packages stacktrace - depth 2', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error')
  err.pkgsStack = [
    {
      id: 'registry.npmjs.org/foo/1.0.0',
      name: 'foo',
      version: '1.0.0',
    },
    {
      id: 'registry.npmjs.org/bar/1.0.0',
      name: 'bar',
      version: '1.0.0',
    },
  ]
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_SOME_ERROR', 'some error')}
${ERROR_PAD}
${ERROR_PAD}This error happened while installing the dependencies of foo@1.0.0
${ERROR_PAD} at bar@1.0.0`)
    },
  })
})

test('prints error and hint', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error', { hint: 'some hint' })
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(formatErrorCode('ERR_PNPM_SOME_ERROR') + ' ' + `${chalk.red('some error')}

some hint`)
    },
  })
})

test('prints authorization error with auth settings', (done) => {
  const rawConfig = {
    '//foo.bar:_auth': '9876543219',
    '//foo.bar:_authToken': '9876543219',
    '//foo.bar:_password': '9876543219',
    '//foo.bar:username': 'kiss.reka',
    '@foo:registry': 'https://foo.bar',
    _auth: '0123456789',
    _authToken: '0123456789',
    _password: '0123456789',
    'always-auth': false,
    username: 'nagy.gabor',
  }
  const output$ = toOutput$({
    context: { argv: ['install'], config: { rawConfig } as any }, // eslint-disable-line
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('FETCH_401', 'some error', { hint: 'some hint' })
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_FETCH_401', 'some error')}
${ERROR_PAD}
${ERROR_PAD}some hint
${ERROR_PAD}
${ERROR_PAD}These authorization settings were found:
${ERROR_PAD}//foo.bar:_auth=9876[hidden]
${ERROR_PAD}//foo.bar:_authToken=9876[hidden]
${ERROR_PAD}//foo.bar:_password=[hidden]
${ERROR_PAD}//foo.bar:username=kiss.reka
${ERROR_PAD}@foo:registry=https://foo.bar
${ERROR_PAD}_auth=0123[hidden]
${ERROR_PAD}_authToken=0123[hidden]
${ERROR_PAD}_password=[hidden]
${ERROR_PAD}always-auth=false
${ERROR_PAD}username=nagy.gabor`)
    },
  })
})

test('prints authorization error without auth settings, where there are none', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'], config: { rawConfig: {} } as any }, // eslint-disable-line
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('FETCH_401', 'some error', { hint: 'some hint' })
  logger.error(err, err)

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${formatError('ERR_PNPM_FETCH_401', 'some error')}
${ERROR_PAD}
${ERROR_PAD}some hint
${ERROR_PAD}
${ERROR_PAD}No authorization settings were found in the configs.
${ERROR_PAD}Try to log in to the registry by running "pnpm login"
${ERROR_PAD}or add the auth tokens manually to the ~/.npmrc file.`)
    },
  })
})
