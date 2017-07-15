"use strict";

var isValue = require("./is-value");

module.exports = function (value) {
	return isValue(value) && !isNaN(value);
};
