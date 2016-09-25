/// <reference path="tape.d.ts" />
declare module 'tape-promise' {
    export = tapePromise;
}

declare function tapePromise(tape: any): (name: string, cb: tape.TestCase) => void;
declare function tapePromise(tape: any): (name: string, opts: tape.TestOptions, cb: tape.TestCase) => void;
declare function tapePromise(tape: any): (cb: tape.TestCase) => void;
declare function tapePromise(tape: any): (opts: tape.TestOptions, cb: tape.TestCase) => void;
