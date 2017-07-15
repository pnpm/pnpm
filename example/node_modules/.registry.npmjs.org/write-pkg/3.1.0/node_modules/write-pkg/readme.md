# write-pkg [![Build Status](https://travis-ci.org/sindresorhus/write-pkg.svg?branch=master)](https://travis-ci.org/sindresorhus/write-pkg)

> Write a `package.json` file

Writes atomically and creates directories for you as needed. Sorts dependencies when writing. Preserves the indentation if the file already exists.


## Install

```
$ npm install write-pkg
```


## Usage

```js
const path = require('path');
const writePkg = require('write-pkg');

writePkg({foo: true}).then(() => {
	console.log('done');
});

writePkg(__dirname, {foo: true}).then(() => {
	console.log('done');
});

writePkg(path.join('unicorn', 'package.json'), {foo: true}).then(() => {
	console.log('done');
});
```


## API

### writePkg([path], data)

Returns a `Promise`.

### writePkg.sync([path], data)

#### path

Type: `string`<br>
Default: `process.cwd()`

Path to where the `package.json` file should be written or its directory.


## Related

- [read-pkg](https://github.com/sindresorhus/read-pkg) - Read a `package.json` file
- [write-json-file](https://github.com/sindresorhus/write-json-file) - Stringify and write JSON to a file atomically


## License

MIT © [Sindre Sorhus](https://sindresorhus.com)
