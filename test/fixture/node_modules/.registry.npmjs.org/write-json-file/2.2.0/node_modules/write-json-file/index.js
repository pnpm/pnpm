'use strict';
const path = require('path');
const fs = require('graceful-fs');
const writeFileAtomic = require('write-file-atomic');
const sortKeys = require('sort-keys');
const makeDir = require('make-dir');
const pify = require('pify');
const detectIndent = require('detect-indent');

const init = (fn, fp, data, opts) => {
	if (!fp) {
		throw new TypeError('Expected a filepath');
	}

	if (data === undefined) {
		throw new TypeError('Expected data to stringify');
	}

	opts = Object.assign({
		indent: '\t',
		sortKeys: false
	}, opts);

	if (opts.sortKeys) {
		data = sortKeys(data, {
			deep: true,
			compare: typeof opts.sortKeys === 'function' && opts.sortKeys
		});
	}

	return fn(fp, data, opts);
};

const readFile = fp => pify(fs.readFile)(fp, 'utf8').catch(() => {});

const main = (fp, data, opts) => {
	return (opts.detectIndent ? readFile(fp) : Promise.resolve())
		.then(str => {
			const indent = str ? detectIndent(str).indent : opts.indent;
			const json = JSON.stringify(data, opts.replacer, indent);

			return pify(writeFileAtomic)(fp, `${json}\n`, {mode: opts.mode});
		});
};

const mainSync = (fp, data, opts) => {
	let indent = opts.indent;

	if (opts.detectIndent) {
		try {
			const file = fs.readFileSync(fp, 'utf8');
			indent = detectIndent(file).indent;
		} catch (err) {
			if (err.code !== 'ENOENT') {
				throw err;
			}
		}
	}

	const json = JSON.stringify(data, opts.replacer, indent);

	return writeFileAtomic.sync(fp, `${json}\n`, {mode: opts.mode});
};

module.exports = (fp, data, opts) => {
	return makeDir(path.dirname(fp), {fs})
		.then(() => init(main, fp, data, opts));
};

module.exports.sync = (fp, data, opts) => {
	makeDir.sync(path.dirname(fp), {fs});
	init(mainSync, fp, data, opts);
};
