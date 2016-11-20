declare module 'tape-promise' {
    import tape = require('tape')
    export = tapePromise;

    function tapePromise(tape: any): (name: string, cb: tape.TestCase) => void;
    function tapePromise(tape: any): (name: string, opts: tape.TestOptions, cb: tape.TestCase) => void;
    function tapePromise(tape: any): (cb: tape.TestCase) => void;
    function tapePromise(tape: any): (opts: tape.TestOptions, cb: tape.TestCase) => void;
}
