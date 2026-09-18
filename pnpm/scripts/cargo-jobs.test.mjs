import assert from 'node:assert/strict'
import { test } from 'node:test'
import { jobsForMachine, parallelismEnv } from './cargo-jobs.mjs'

test('caps a host that has more cores than memory to spend on them', () => {
  assert.equal(jobsForMachine(32, 60 * 1024 ** 3), 15)
})

test('leaves a CI runner at its core count', () => {
  assert.equal(jobsForMachine(4, 16 * 1024 ** 3), 4)
  assert.equal(jobsForMachine(12, 64 * 1024 ** 3), 12)
})

test('always allows one job', () => {
  assert.equal(jobsForMachine(1, 1024 ** 3), 1)
})

test('test threads follow the build jobs the caller asked for', () => {
  assert.deepEqual(parallelismEnv({ CARGO_BUILD_JOBS: '4' }), {
    CARGO_BUILD_JOBS: '4',
    NEXTEST_TEST_THREADS: '4',
  })
})

test('an explicit test-thread count wins for the test phase alone', () => {
  const env = parallelismEnv({ CARGO_BUILD_JOBS: '4', NEXTEST_TEST_THREADS: '2' })
  assert.deepEqual(env, { CARGO_BUILD_JOBS: '4', NEXTEST_TEST_THREADS: '2' })
})

test('derives both from the machine when neither is set', () => {
  const env = parallelismEnv({})
  assert.equal(env.NEXTEST_TEST_THREADS, env.CARGO_BUILD_JOBS)
  assert.ok(Number.parseInt(env.CARGO_BUILD_JOBS, 10) >= 1)
})
