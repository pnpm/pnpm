'use strict';
module.exports = function (n) {
	return toString.call(n) === '[object Number]' && n > 0;
};
