import { describe, expect, it } from '@jest/globals'

import { parsePatchFile } from '../src/parsePatch.js'

const patch = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,5 +1,5 @@
 this
 is
 
-a
+
 file
`
const invalidHeaders1 = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,5 +1,4 @@
 this
 is
 
-a
+
 file
`

const invalidHeaders2 = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,4 +1,5 @@
 this
 is
 
-a
+
 file
`

const invalidHeaders3 = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,0 +1,5 @@
 this
 is
 
-a
+
 file
`
const invalidHeaders4 = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,5 +1,0 @@
 this
 is
 
-a
+
 file
`

const invalidHeaders5 = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,5 +1,5@@
 this
 is
 
-a
+
 file
`

const accidentalBlankLine = `diff --git a/banana.ts b/banana.ts
index 2de83dd..842652c 100644
--- a/banana.ts
+++ b/banana.ts
@@ -1,5 +1,5 @@
 this
 is

-a
+
 file
`

const crlfLineBreaks = `diff --git a/banana.ts b/banana.ts
new file mode 100644
index 0000000..3e1267f
--- /dev/null
+++ b/banana.ts
@@ -0,0 +1 @@
+this is a new file
`.replaceAll('\n', '\r\n')

const modeChangeAndModifyAndRename = `diff --git a/numbers.txt b/banana.txt
old mode 100644
new mode 100755
similarity index 96%
rename from numbers.txt
rename to banana.txt
index fbf1785..92d2c5f
--- a/numbers.txt
+++ b/banana.txt
@@ -1,4 +1,4 @@
-one
+ne
 
 two
 
`

const oldStylePatch = `patch-package
--- a/node_modules/graphql/utilities/assertValidName.js
+++ b/node_modules/graphql/utilities/assertValidName.js
@@ -41,10 +41,11 @@ function assertValidName(name) {
  */
 function isValidNameError(name, node) {
   !(typeof name === 'string') ? (0, _invariant2.default)(0, 'Expected string') : void 0;
-  if (name.length > 1 && name[0] === '_' && name[1] === '_') {
-    return new _GraphQLError.GraphQLError('Name "' + name + '" must not begin with "__", which is reserved by ' + 'GraphQL introspection.', node);
-  }
+  // if (name.length > 1 && name[0] === '_' && name[1] === '_') {
+  //   return new _GraphQLError.GraphQLError('Name "' + name + '" must not begin with "__", which is reserved by ' + 'GraphQL introspection.', node);
+  // }
   if (!NAME_RX.test(name)) {
     return new _GraphQLError.GraphQLError('Names must match /^[_a-zA-Z][_a-zA-Z0-9]*$/ but "' + name + '" does not.', node);
   }
+
 }
\\ No newline at end of file
--- a/node_modules/graphql/utilities/assertValidName.mjs
+++ b/node_modules/graphql/utilities/assertValidName.mjs
@@ -29,9 +29,9 @@ export function assertValidName(name) {
  */
 export function isValidNameError(name, node) {
   !(typeof name === 'string') ? invariant(0, 'Expected string') : void 0;
-  if (name.length > 1 && name[0] === '_' && name[1] === '_') {
-    return new GraphQLError('Name "' + name + '" must not begin with "__", which is reserved by ' + 'GraphQL introspection.', node);
-  }
+  // if (name.length > 1 && name[0] === '_' && name[1] === '_') {
+  //   return new GraphQLError('Name "' + name + '" must not begin with "__", which is reserved by ' + 'GraphQL introspection.', node);
+  // }
   if (!NAME_RX.test(name)) {
     return new GraphQLError('Names must match /^[_a-zA-Z][_a-zA-Z0-9]*$/ but "' + name + '" does not.', node);
   }
`

describe('parsePatchFile()', () => {
  it('parses a simple patch', () => {
    expect(parsePatchFile(patch)).toMatchSnapshot()
  })

  it('fails when a hunk header does not match its body', () => {
    expect(() => parsePatchFile(invalidHeaders1)).toThrow('hunk header integrity check failed')
    expect(() => parsePatchFile(invalidHeaders2)).toThrow('hunk header integrity check failed')
    expect(() => parsePatchFile(invalidHeaders3)).toThrow('hunk header integrity check failed')
    expect(() => parsePatchFile(invalidHeaders4)).toThrow('hunk header integrity check failed')
    expect(() => parsePatchFile(invalidHeaders5)).toThrow('Bad header line')
  })

  it('reads a blank line in a hunk as context', () => {
    expect(parsePatchFile(accidentalBlankLine)).toEqual(parsePatchFile(patch))
  })

  it('parses a patch with CRLF line breaks', () => {
    expect(parsePatchFile(crlfLineBreaks)).toMatchSnapshot()
  })

  it('parses a mode change, a modification, and a rename of one file', () => {
    expect(parsePatchFile(modeChangeAndModifyAndRename)).toMatchSnapshot()
  })

  it('parses a patch without diff --git lines', () => {
    expect(parsePatchFile(oldStylePatch)).toMatchSnapshot()
  })

  it('splits a diff --git line at the first " b/"', () => {
    const [part] = parsePatchFile('diff --git a/x b/y b/z  \r\nnew file mode 100644\n')
    expect(part).toStrictEqual({ type: 'file creation', path: 'y b/z', mode: 0o644, hunk: null, hash: null })
  })

  it('splits a diff --git line between two equal paths that contain " b/"', () => {
    const [part] = parsePatchFile('diff --git a/foo b/bar.js b/foo b/bar.js\ndeleted file mode 100644\n')
    expect(part).toStrictEqual({ type: 'file deletion', path: 'foo b/bar.js', mode: 0o644, hunk: null, hash: null })
  })

  it('rejects a diff --git line without " b/"', () => {
    expect(() => parsePatchFile('diff --git a/x c/y\n')).toThrow('Bad diff line: diff --git a/x c/y')
  })

  it('parses a diff --git line ending in a long run of whitespace in linear time', () => {
    const line = `diff --git a/x b/${' '.repeat(200_000)}y`
    const start = performance.now()
    expect(parsePatchFile(`${line}\nnew file mode 100644\n`)).toHaveLength(1)
    expect(performance.now() - start).toBeLessThan(1000)
  })
})
