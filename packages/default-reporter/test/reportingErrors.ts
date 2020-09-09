import { toOutput$ } from '@pnpm/default-reporter'
import PnpmError from '@pnpm/error'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import { map, take } from 'rxjs/operators'
import path = require('path')
import chalk = require('chalk')
import loadJsonFile = require('load-json-file')
import normalizeNewline = require('normalize-newline')
import StackTracey = require('stacktracey')
import test = require('tape')

const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')

test('prints generic error', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  logger.error(err)

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('some error')}
${new StackTracey(err.stack).pretty as string}`)
    },
  })
})

test('prints generic error when recursive install fails', t => {
  const output$ = toOutput$({
    context: { argv: ['recursive'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  err['prefix'] = '/home/src/'
  logger.error(err, err)

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `/home/src/:
${ERROR} ${chalk.red('some error')}
${new StackTracey(err.stack).pretty as string}`)
    },
  })
})

test('prints no matching version error when many dist-tags exist', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('No matching version found for pnpm@1000.0.0')}

The latest release of pnpm is "2.4.0".

Other releases are:
  * stable: 2.2.2
  * next: 2.4.0
  * latest-1: 1.43.1

If you need the full list of all 281 published versions run "$ pnpm view pnpm versions".`)
    },
  })

  const err = new PnpmError('NO_MATCHING_VERSION', 'No matching version found for pnpm@1000.0.0')
  err['packageMeta'] = await loadJsonFile(path.join(__dirname, 'pnpm-meta.json'))
  logger.error(err, err)
})

test('prints no matching version error when only the latest dist-tag exists', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('No matching version found for is-positive@1000.0.0')}

The latest release of is-positive is "3.1.0".

If you need the full list of all 4 published versions run "$ pnpm view is-positive versions".`)
    },
  })

  const err = new PnpmError('NO_MATCHING_VERSION', 'No matching version found for is-positive@1000.0.0')
  err['packageMeta'] = await loadJsonFile(path.join(__dirname, 'is-positive-meta.json'))
  logger.error(err, err)
})

test('prints suggestions when an internet-connection related error happens', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Actual size (99) of tarball (https://foo) did not match the one specified in \'Content-Length\' header (100)')}

Seems like you have internet connection issues.
Try running the same command again.
If that doesn't help, try one of the following:

- Set a bigger value for the \`fetch-retries\` config.
    To check the current value of \`fetch-retries\`, run \`pnpm get fetch-retries\`.
    To set a new value, run \`pnpm set fetch-retries <number>\`.

- Set \`network-concurrency\` to 1.
    This change will slow down installation times, so it is recommended to
    delete the config once the internet connection is good again: \`pnpm config delete network-concurrency\`

NOTE: You may also override configs via flags.
For instance, \`pnpm install --fetch-retries 5 --network-concurrency 1\``)
    },
  })

  const err = new PnpmError('BAD_TARBALL_SIZE', 'Actual size (99) of tarball (https://foo) did not match the one specified in \'Content-Length\' header (100)')
  err['expectedSize'] = 100
  err['receivedSize'] = 99
  logger.error(err, err)
})

test('prints test error', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['run', 'test'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Test failed. See above for more details.')}`)
    },
  })

  const err = new Error('Tests failed')
  err['stage'] = 'test'
  err['code'] = 'ELIFECYCLE'
  logger.error(err, err)
})

test('prints command error with exit code', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['run', 'lint'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Command failed with exit code 100.')}`)
    },
  })

  const err = new Error('Command failed')
  err['errno'] = 100
  err['stage'] = 'lint'
  err['code'] = 'ELIFECYCLE'
  logger.error(err, err)
})

test('prints command error without exit code', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['run', 'lint'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Command failed.')}`)
    },
  })

  const err = new Error('Command failed')
  err['stage'] = 'lint'
  err['code'] = 'ELIFECYCLE'
  logger.error(err, err)
})

test('prints unsupported pnpm version error', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Your pnpm version is incompatible with "/home/zoltan/project".')}

Expected version: 2
Got: 3.0.0

This is happening because the package's manifest has an engines.pnpm field specified.
To fix this issue, install the required pnpm version globally.

To install the latest version of pnpm, run "pnpm i -g pnpm".
To check your pnpm version, run "pnpm -v".`)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { pnpm: '2' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints unsupported Node version error', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Your Node version is incompatible with "/home/zoltan/project".')}

Expected version: >=12
Got: 10.0.0

This is happening because the package's manifest has an engines.node field specified.
To fix this issue, install the required Node version.`)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { node: '>=12' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints unsupported pnpm and Node versions error', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('Your pnpm version is incompatible with "/home/zoltan/project".')}

Expected version: 2
Got: 3.0.0

This is happening because the package's manifest has an engines.pnpm field specified.
To fix this issue, install the required pnpm version globally.

To install the latest version of pnpm, run "pnpm i -g pnpm".
To check your pnpm version, run "pnpm -v".` + '\n\n' + `\
${ERROR} ${chalk.red('Your Node version is incompatible with "/home/zoltan/project".')}

Expected version: >=12
Got: 10.0.0

This is happening because the package's manifest has an engines.node field specified.
To fix this issue, install the required Node version.`)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { pnpm: '2', node: '>=12' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints error even if the error object not passed in through the message object', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error')
  logger.error(err)

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}`)
    },
  })
})

test('prints error without packages stacktrace when pkgsStack is empty', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error')
  err.pkgsStack = []
  logger.error(err, err)

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}`)
    },
  })
})

test('prints error with packages stacktrace - depth 1 and hint', t => {
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

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}
This error happened while installing the dependencies of foo@1.0.0
hint`)
    },
  })
})

test('prints error with packages stacktrace - depth 2', t => {
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

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}
This error happened while installing the dependencies of foo@1.0.0
 at bar@1.0.0`)
    },
  })
})

test('prints error and hint', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('SOME_ERROR', 'some error', { hint: 'some hint' })
  logger.error(err, err)

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}
some hint`)
    },
  })
})

test('prints authorization error with auth settings', t => {
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

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}
some hint

These authorization settings were found:
//foo.bar:_auth=9876[hidden]
//foo.bar:_authToken=9876[hidden]
//foo.bar:_password=[hidden]
//foo.bar:username=kiss.reka
@foo:registry=https://foo.bar
_auth=0123[hidden]
_authToken=0123[hidden]
_password=[hidden]
always-auth=false
username=nagy.gabor`)
    },
  })
})

test('prints authorization error without auth settings, where there are none', t => {
  const output$ = toOutput$({
    context: { argv: ['install'], config: { rawConfig: {} } as any }, // eslint-disable-line
    streamParser: createStreamParser(),
  })

  const err = new PnpmError('FETCH_401', 'some error', { hint: 'some hint' })
  logger.error(err, err)

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + `${chalk.red('some error')}
some hint

No authorization settings were found in the configs.
Try to log in to the registry by running "pnpm login"
or add the auth tokens manually to the ~/.npmrc file.`)
    },
  })
})
