declare module 'tape-promise' {
    import tape = require('tape')
    export = tapePromise;

    function tapePromise(tape: any): (name: string, cb: tape.TestCase) => void;
    function tapePromise(tape: any): (name: string, opts: tape.TestOptions, cb: tape.TestCase) => void;
    function tapePromise(tape: any): (cb: tape.TestCase) => void;
    function tapePromise(tape: any): (opts: tape.TestOptions, cb: tape.TestCase) => void;
}

declare module 'jest-t-assert' {
  import tape = require('tape')
  export default test;

  function test (name: string, cb: tape.TestCase): void;
  function test (name: string, opts: tape.TestOptions, cb: tape.TestCase): void;
  function test (cb: tape.TestCase): void;
  function test (opts: tape.TestOptions, cb: tape.TestCase): void;
}
