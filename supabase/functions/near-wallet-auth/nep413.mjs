// NEP-413: SHA-256(Borsh({ tag: 2**31 + 413, message, nonce[32], recipient,
// callbackUrl: Option<string> })). Only Ed25519 FullAccess keys are accepted.
// WebCrypto performs all hashing/signature operations; no custom cryptography.
const encoder = new TextEncoder();
const alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

export function base64(bytes) {
  return btoa(String.fromCharCode(...bytes));
}

export function decodeBase64(value, length) {
  if (typeof value !== 'string' || value.length > 128) throw new Error('Invalid encoding');
  const bytes = Uint8Array.from(atob(value), (char) => char.charCodeAt(0));
  if (bytes.length !== length || base64(bytes) !== value) throw new Error('Invalid encoding');
  return bytes;
}

function publicKeyBytes(value) {
  if (typeof value !== 'string' || !/^ed25519:[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value)) {
    throw new Error('An Ed25519 public key is required');
  }
  const encoded = value.slice(8);
  let number = 0n;
  for (const character of encoded) number = number * 58n + BigInt(alphabet.indexOf(character));
  const bytes = [];
  while (number > 0n) { bytes.unshift(Number(number & 255n)); number >>= 8n; }
  for (const character of encoded) {
    if (character !== '1') break;
    bytes.unshift(0);
  }
  if (bytes.length !== 32) throw new Error('Invalid public key length');
  return Uint8Array.from(bytes);
}

function u32(number) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setUint32(0, number, true);
  return bytes;
}

function string(value) {
  const bytes = encoder.encode(value);
  return [u32(bytes.length), bytes];
}

export function serializeNep413({ message, nonce, recipient, callbackUrl }) {
  const parts = [u32(2147484061), ...string(message), decodeBase64(nonce, 32), ...string(recipient),
    new Uint8Array([callbackUrl ? 1 : 0]), ...(callbackUrl ? string(callbackUrl) : [])];
  const result = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) { result.set(part, offset); offset += part.length; }
  return result;
}

export async function sha256Hex(value) {
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', encoder.encode(value)));
  return Array.from(hash, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export async function verifyNep413(challenge, signed) {
  try {
    if (signed?.accountId !== challenge.accountId) return false;
    const key = await crypto.subtle.importKey('raw', publicKeyBytes(signed.publicKey), 'Ed25519', false, ['verify']);
    const digest = await crypto.subtle.digest('SHA-256', serializeNep413(challenge));
    return await crypto.subtle.verify('Ed25519', key, decodeBase64(signed.signature, 64), digest);
  } catch { return false; }
}
