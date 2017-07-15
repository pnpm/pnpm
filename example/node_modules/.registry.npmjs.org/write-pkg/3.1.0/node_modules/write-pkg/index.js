'use strict';
const path = require('path');
const writeJsonFile = require('write-json-file');
const sortKeys = require('sort-keys');

const opts = {detectIndent: true};

const dependencyKeys = new Set([
	'dependencies',
	'devDependencies',
	'optionalDependencies',
	'peerDependencies'
]);

function normalize(pkg) {
	const ret = {};

	for (const key of Object.keys(pkg)) {
		if (!dependencyKeys.has(key)) {
			ret[key] = pkg[key];
		} else if (Object.keys(pkg[key]).length !== 0) {
			ret[key] = sortKeys(pkg[key]);
		}
	}

	return ret;
}

module.exports = (fp, data) => {
	if (typeof fp !== 'string') {
		data = fp;
		fp = '.';
	}

	fp = path.basename(fp) === 'package.json' ? fp : path.join(fp, 'package.json');

	data = normalize(data);

	return writeJsonFile(fp, data, opts);
};

module.exports.sync = (fp, data) => {
	if (typeof fp !== 'string') {
		data = fp;
		fp = '.';
	}

	fp = path.basename(fp) === 'package.json' ? fp : path.join(fp, 'package.json');

	data = normalize(data);

	writeJsonFile.sync(fp, data, opts);
};
