import { extractComments, insertComments } from '@pnpm/text.comments-parser'

test('extract and insert JSON5 comments', () => {
  const json5WithComments = `/* This is an example of a package.json5 file with comments. */
{
    /* pnpm should keep comments at the same indentation level */
    name: 'foo',
    version: '1.0.0', // it should keep in-line comments on the same line
    // It should allow in-line comments with no other content
    type: 'commonjs',
}
/* And it should preserve comments at the end of the file. Note no newline. */`
  const { comments } = extractComments(json5WithComments)
  expect(insertComments(`{
    name: 'foo',
    version: '1.0.0',
    type: 'commonjs',
}`, comments!))
})
