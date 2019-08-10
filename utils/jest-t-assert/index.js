'use strict';

Object.defineProperty(exports, "__esModule", {
  value: true
});
const t = {
  pass: () => expect(true).toBe(true),
  fail: () => {
    throw new Error('Test failed via `t.fail()`');
  },
  ok: actual => expect(actual).toBeTruthy(),
  notOk: actual => expect(actual).toBeFalsy(),
  equal: (actual, expected) => expect(actual).toBe(expected),
  not: (actual, expected) => expect(actual).not.toBe(expected),
  deepEqual: (actual, expected) => expect(actual).toEqual(expected),
  notDeepEqual: (actual, expected) => expect(actual).not.toEqual(expected),
  throws: (actual, error) => expect(actual).toThrow(error),
  notThrows: actual => expect(actual).not.toThrow(),
  regex: (actual, _regex) => expect(actual).toMatch(_regex),
  notRegex: (actual, regex) => expect(actual).not.toMatch(regex),
  plan: count => expect.assertions(count),
  snapshot: actual => expect(actual).toMatchSnapshot(),
  end: () => {},
  skip: () => {},
  comment: () => {},
};

exports.t = t;
function testArgs(args) {
  if (typeof args[0] === 'function') {
    return { message: '', fn: args[0], timeout: args[1] };
  }

  return {
    message: args[0],
    fn: args[1],
    timeout: args[2]
  };
}

function runTest(testFn, args) {
  var _testArgs = testArgs(args);

  const message = _testArgs.message,
        fn = _testArgs.fn,
        timeout = _testArgs.timeout;

  if (typeof fn === 'function') {
    testFn(message, () => fn(t), timeout);
  } else {
    testFn(message, () => {
      throw new TypeError(`Expected a function - got ${typeof fn}`);
    });
  }
}

function runTestWithCallback(testFn, args) {
  var _testArgs2 = testArgs(args);

  const message = _testArgs2.message,
        fn = _testArgs2.fn,
        timeout = _testArgs2.timeout;

  if (typeof fn === 'function') {
    testFn(message, done => fn(Object.assign({ end: done }, t)), timeout);
  } else {
    testFn(message, () => {
      throw new TypeError(`Expected a function - got ${typeof fn}`);
    });
  }
}

function runHook(hookFn, fn) {
  hookFn(() => fn(t));
}

function runHookWithCallback(hookFn, fn) {
  hookFn(done => fn(Object.assign({ end: done }, t)));
}

function tTest(...args) {
  runTest(test, args);
}

tTest.cb = (...args) => runTestWithCallback(test, args);
tTest.only = (...args) => runTest(test.only, args);
tTest.only.cb = (...args) => runTestWithCallback(test.only, args);
tTest.cb.only = tTest.only.cb;
tTest.skip = (...args) => runTest(test.skip, args);
tTest.skip.cb = (...args) => runTestWithCallback(test.skip, args);
tTest.cb.skip = tTest.skip.cb;
tTest.after = fn => runHook(afterAll, fn);
tTest.after.cb = fn => runHookWithCallback(afterAll, fn);
tTest.cb.after = tTest.after.cb;
tTest.afterEach = fn => runHook(afterEach, fn);
tTest.afterEach.cb = fn => runHookWithCallback(afterEach, fn);
tTest.cb.afterEach = tTest.afterEach.cb;
tTest.before = fn => runHook(beforeAll, fn);
tTest.before.cb = fn => runHookWithCallback(beforeAll, fn);
tTest.cb.before = tTest.before.cb;
tTest.beforeEach = fn => runHook(beforeEach, fn);
tTest.beforeEach.cb = fn => runHookWithCallback(beforeEach, fn);
tTest.cb.beforeEach = tTest.beforeEach.cb;

exports.default = tTest;
