# jest-t-assert

[![Build Status][travis-badge]][travis]
[![npm][npm-badge]][npm-link]

Use [tape][tape] style assertions with [Jest][jest]. The assertions are very
similar to [AVA][ava].

*If you just want to migrate from AVA to Jest you should use
[jest-codemods][codemods] instead.*

## Usage

```javascript
import test from 'jest-t-assert';

test('a test case', t => {
  t.true(2 < 10);
  t.truthy(1);
  t.not(2, '2');
  t.deepEqual([1, 2], [1, 2]);
  t.throws(() => {
    throw new Error('This function throws');
  });
  t.snapshot({ a: 1, b: 'ok' });
});
```

You can also import `t` directly if you only want to use the assertions without
the `test` wrapper. This doesn't break existing tests that use Jest's callback
mode, where you call `done()`, which is passed as the first argument to the
test function.

```javascript
import { t } from 'jest-t-assert';

test('only using the assertions from `t`', () => {
  t.true(2 < 10);
  t.truthy(1);
  t.not(2, '2');
  t.deepEqual([1, 2], [1, 2]);
  t.throws(() => {
    throw new Error('This function throws');
  });
  t.snapshot({ a: 1, b: 'ok' });
});
```

### Asynchronous tests

Jest supports returning a promise from the test, so you can do the assertions in
the `.then` calls.

```javascript
import test from 'jest-t-assert';

test('promise resolves with the correct value', t => {
  return Promise.resolve('value').then(val => t.is(val, 'value'));
});

test('using async/await', async t => {
  const val = await Promise.resolve('value');
  t.is(val, 'value');
  // await directly
  t.is(await Promise.resolve('value'), 'value');
});
```

If you need to use the callback mode you have to use `test.cb`, which
requires you to end the test manually by calling `t.end()`. It is highly
recommended to use promises and `async / await` instead of the callback mode.

```javascript
import test from 'jest-t-assert';

test.cb('a callback', t => {
  t.plan(1);
  setTimeout(() => {
    t.pass();
    t.end();
  }, 2000);
});
```

## API

### `test([title], fn, [timeout])`

Run a test. `fn` will receive `t` as the first argument, which provides the
assertions. The `timeout` specifies how long (in milliseconds) to wait until the
test is aborted.

#### `test.cb([title], fn, [timeout])`

Run a test in callback mode. `fn` will receive `t`, which provides the usual
assertions and additionally `t.end()`, which must be called to manually end the
test. `.cb` can be chained with any of modifiers listed below.  The `timeout`
specifies how long (in milliseconds) to wait until the test is aborted.

#### `test.only([title], fn, [timeout])`

Only run this test. All other tests in the test suite are skipped.
The `timeout` specifies how long (in milliseconds) to wait until the test is
aborted.

#### `test.skip([title], fn, [timeout])`

Skip the test. Useful to avoid a failing test without removing it or commenting
it out.

#### `test.after(fn)`

Runs `fn` after all tests in the test suite have completed.

#### `test.afterEach(fn)`

Runs `fn` after each test in the test suite has completed.

#### `test.before(fn)`

Runs `fn` before any test in the test suite is run.

#### `test.beforeEach(fn)`

Runs `fn` before each test in the test suite is run.

### `t`

The argument passed to the callback of `test`, which is also exported as `t`.

#### `t.end()`

End the test. This is only available when using `test.cb`.

#### `t.plan(count)`

Plan how many assertions are used in the test. The test fails if the number of
assertions differs from `count`.

#### `t.pass()`

An assertion that always passes.

#### `t.fail()`

An assertion that always fails.

#### `t.true(actual)`

Assert that `actual` is `true`.

#### `t.false(actual)`

Assert that `actual` is `false`.

#### `t.truthy(actual)`

Assert that `actual` is truthy.

#### `t.falsy(actual)`

Assert that `actual` is falsy.

#### `t.is(actual, expected)`

Assert that `actual` is equal to `expected` (based on `===`).

#### `t.not(actual, expected)`

Assert that `actual` is not equal to `expected` (based on `===`). The inverse
of `t.is(actual, expected)`.

#### `t.deepEqual(actual, expected)`

Assert that `actual` is deeply equal to `expected`. This recursively checks the
equality of all fields.

#### `t.notDeepEqual(actual, expected)`

Assert that `actual` is not deeply equal to `expected`. This recursively checks
the equality of all fields. The inverse of `t.deepEqual(actual, expected)`.

#### `t.throws(fn, [error])`

Assert that `fn` throws an error. If `error` is provided, the thrown error must
match it.

#### `t.notThrows(fn)`

Assert that `fn` does not throw an error. The inverse of `t.throws(fn)`.

#### `t.regex(actual, regex)`

Assert that `actual` matches `regex`.

#### `t.notRegex(actual, regex)`

Assert that `actual` does not match `regex`. The inverse of
`t.regex(actual, regex)`.

#### `t.snapshot(actual)`

Assert that `actual` matches the most recent snapshot.

[ava]: https://github.com/avajs/ava
[codemods]: https://github.com/skovhus/jest-codemods
[jest]: https://github.com/facebook/jest
[npm-badge]: https://img.shields.io/npm/v/jest-t-assert.svg?style=flat-square
[npm-link]: https://www.npmjs.com/package/jest-t-assert
[tape]: https://github.com/substack/tape
[travis]: https://travis-ci.org/jungomi/jest-t-assert
[travis-badge]: https://img.shields.io/travis/jungomi/jest-t-assert/master.svg?style=flat-square
