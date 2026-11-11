// Dependency-free validators for the trusted settlement worker.
const { TextDecoder } = require('node:util');
const U128_MAX = (1n << 128n) - 1n;
const U64_MAX = (1n << 64n) - 1n;
const YOCTO_PER_NEAR = 10n ** 24n;
const BASE58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

function check(condition, message) {
  if (!condition) throw new Error(message);
}
function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}
function exactKeys(value, keys) {
  return object(value) && Object.keys(value).length === keys.length &&
    keys.every((key) => Object.hasOwn(value, key));
}
function accountId(value) {
  return typeof value === 'string' && value.length >= 2 && value.length <= 64 &&
    /^[a-z0-9]+([._-][a-z0-9]+)*$/.test(value);
}
function hash(value) {
  if (typeof value !== 'string' || !/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value)) return false;
  let n = 0n;
  for (const char of value) n = n * 58n + BigInt(BASE58.indexOf(char));
  let bytes = 0;
  while (n > 0n) { bytes++; n >>= 8n; }
  return bytes + value.match(/^1*/)[0].length === 32;
}
function uint(value, max = U128_MAX, positive = false) {
  check(typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value) && value.length <= 39,
    'Expected a canonical unsigned integer string');
  const number = BigInt(value);
  check(number <= max && (!positive || number > 0n), 'Integer out of range');
  return number;
}
function height(value) { return Number.isSafeInteger(value) && value >= 0; }
function utf8(bytes) { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
function base64(value) {
  check(typeof value === 'string' && value.length <= 2_000_000 &&
    /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value), 'Invalid base64');
  const bytes = Buffer.from(value, 'base64');
  check(bytes.toString('base64') === value, 'Noncanonical base64');
  return bytes;
}

// Rust views include raw u128/u64 JSON numbers. Preserve every number as text
// and reject duplicate keys instead of rounding or silently replacing evidence.
function chainJson(source) {
  check(typeof source === 'string' && source.length <= 2_000_000, 'Invalid JSON input');
  let i = 0;
  const space = () => { while (/[\x20\t\r\n]/.test(source[i] || '\0')) i++; };
  function string() {
    const start = i++;
    while (i < source.length) {
      const char = source[i++];
      if (char === '\\') i++;
      else if (char === '"') return JSON.parse(source.slice(start, i));
    }
    throw new Error('Unterminated JSON string');
  }
  function value(depth) {
    check(depth <= 64, 'JSON nesting limit');
    space();
    if (source[i] === '"') return string();
    const open = source[i];
    if (open === '{' || open === '[') {
      const isObject = open === '{';
      const close = isObject ? '}' : ']';
      const result = isObject ? Object.create(null) : [];
      i++; space();
      if (source[i] === close) { i++; return result; }
      for (;;) {
        space();
        if (isObject) {
          check(source[i] === '"', 'Invalid JSON object key');
          const key = string();
          check(!Object.hasOwn(result, key), 'Duplicate JSON key');
          space(); check(source[i++] === ':', 'Missing JSON colon');
          result[key] = value(depth + 1);
        } else result.push(value(depth + 1));
        space();
        if (source[i] === close) { i++; return result; }
        check(source[i++] === ',', 'Missing JSON comma');
      }
    }
    for (const [literal, parsed] of [['true', true], ['false', false], ['null', null]]) {
      if (source.startsWith(literal, i)) { i += literal.length; return parsed; }
    }
    const number = source.slice(i).match(/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/);
    check(number, 'Invalid JSON value');
    i += number[0].length;
    return number[0];
  }
  const result = value(0);
  space(); check(i === source.length, 'Trailing JSON data');
  return result;
}
function functionCall(actions, method) {
  check(Array.isArray(actions) && actions.length === 1 && exactKeys(actions[0], ['FunctionCall']),
    'Expected one FunctionCall action');
  const call = actions[0].FunctionCall;
  check(exactKeys(call, ['method_name', 'args', 'gas', 'deposit']) && call.method_name === method &&
    Number.isSafeInteger(call.gas) && call.gas > 0, 'Unexpected function call');
  uint(call.deposit);
  return { ...call, json: chainJson(utf8(base64(call.args))) };
}
module.exports = {
  U128_MAX, U64_MAX, YOCTO_PER_NEAR, check, object, exactKeys, accountId,
  hash, uint, height, utf8, base64, chainJson, functionCall,
};
