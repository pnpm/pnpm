'use strict';

module.exports = function (n) {
	if (typeof n !== 'number') {
		throw new TypeError('Expected a number');
	}

	return n < 0;
};
