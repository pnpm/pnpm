import { toOutput$ } from '@pnpm/default-reporter'
import PnpmError from '@pnpm/error'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import chalk = require('chalk')
import { stripIndent, stripIndents } from 'common-tags'
import loadJsonFile = require('load-json-file')
import normalizeNewline = require('normalize-newline')
import path = require('path')
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        ${ERROR} ${chalk.red('some error')}
        ${new StackTracey(err.stack).pretty}
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        /home/src/:
        ${ERROR} ${chalk.red('some error')}
        ${new StackTracey(err.stack).pretty}
      `)
    },
  })
})

test('prints no matching version error when many dist-tags exist', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('No matching version found for pnpm@1000.0.0')}

        The latest release of pnpm is "2.4.0".

        Other releases are:
          * stable: 2.2.2
          * next: 2.4.0
          * latest-1: 1.43.1

        If you need the full list of all 281 published versions run "$ pnpm view pnpm versions".
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('No matching version found for is-positive@1000.0.0')}

        The latest release of is-positive is "3.1.0".

        If you need the full list of all 4 published versions run "$ pnpm view is-positive versions".
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('Actual size (99) of tarball (https://foo) did not match the one specified in \'Content-Length\' header (100)')}

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
        For instance, \`pnpm install --fetch-retries 5 --network-concurrency 1\`
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
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

  output$.take(1).map(normalizeNewline).subscribe({
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

  output$.take(1).map(normalizeNewline).subscribe({
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('Your pnpm version is incompatible with "/home/zoltan/project".')}

        Expected version: 2
        Got: 3.0.0

        This is happening because the package's manifest has an engines.pnpm field specified.
        To fix this issue, install the required pnpm version globally.

        To install the latest version of pnpm, run "pnpm i -g pnpm".
        To check your pnpm version, run "pnpm -v".
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('Your Node version is incompatible with "/home/zoltan/project".')}

        Expected version: >=12
        Got: 10.0.0

        This is happening because the package's manifest has an engines.node field specified.
        To fix this issue, install the required Node version.
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('Your pnpm version is incompatible with "/home/zoltan/project".')}

        Expected version: 2
        Got: 3.0.0

        This is happening because the package's manifest has an engines.pnpm field specified.
        To fix this issue, install the required pnpm version globally.

        To install the latest version of pnpm, run "pnpm i -g pnpm".
        To check your pnpm version, run "pnpm -v".` + '\n\n' + stripIndent`
        ${ERROR} ${chalk.red('Your Node version is incompatible with "/home/zoltan/project".')}

        Expected version: >=12
        Got: 10.0.0

        This is happening because the package's manifest has an engines.node field specified.
        To fix this issue, install the required Node version.
      `)
    },
  })

  const err = new PnpmError('UNSUPPORTED_ENGINE', 'Unsupported pnpm version')
  err['packageId'] = '/home/zoltan/project'
  err['wanted'] = { pnpm: '2', node: '>=12' }
  err['current'] = { pnpm: '3.0.0', node: '10.0.0' }
  logger.error(err, err)
})

test('prints error with packages stacktrace - depth 1', t => {
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
  ]
  logger.error(err, err)

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + stripIndents`
        ${chalk.red('some error')}
        This error happened while installing the dependencies of foo@1.0.0
      `)
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, ERROR + ' ' + stripIndent`
        ${chalk.red('some error')}
        This error happened while installing the dependencies of foo@1.0.0
         at bar@1.0.0
      `)
    },
  })
})
