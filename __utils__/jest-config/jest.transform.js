import { stripTypeScriptTypes } from 'node:module'
import { fileURLToPath } from 'node:url'
import { transformSync } from '@babel/core'

// This file was created referencing:
// https://github.com/jestjs/jest/issues/15443

export default {
  process(sourceText, sourcePath) {
    const code = stripTypeScriptTypes(sourceText, { mode: 'strip' })

    // Using the presence of the DisposableStack global to feature detect
    // whether the current Node.js runtime supports explicit resource
    // management. If it exists, we don't need to do any more work.
    if (typeof DisposableStack !== "undefined") {
      return { code }
    }

    // Node.js 24.0.0 introduces explicit resource management:
    // https://nodejs.org/en/blog/release/v24.0.0#v8-136
    //
    // When running tests on Node.js before version 24, we'll need to transpile
    // the "using" keyword. Otherwise Jest will fail to recognize this syntax
    // and show confusing errors when running some tests.
    //
    // This can be deleted when pnpm no longer needs to support Node.js v22.
    return babel(code, sourcePath);
  }
};

function babel (code, sourceFileName) {
  return transformSync(code, {
    sourceFileName,
    plugins: [fileURLToPath(import.meta.resolve("@babel/plugin-transform-explicit-resource-management"))]
  })
}
