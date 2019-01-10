import { toOutput$ } from '@pnpm/default-reporter'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import chalk from 'chalk'
import { stripIndent, stripIndents } from 'common-tags'
import loadJsonFile from 'load-json-file'
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

  const err = new Error('No matching version found for pnpm@1000.0.0')
  err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
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

  const err = new Error('No matching version found for is-positive@1000.0.0')
  err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
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

  const err = new Error('Actual size (99) of tarball (https://foo) did not match the one specified in \'Content-Length\' header (100)')
  err['code'] = 'ERR_PNPM_BAD_TARBALL_SIZE'
  err['expectedSize'] = 100
  err['receivedSize'] = 99
  logger.error(err, err)
})
