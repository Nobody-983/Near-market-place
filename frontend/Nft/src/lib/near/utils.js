const YOCTO_PER_NEAR = 10n ** 24n;
const MAX_YOCTO = (1n << 128n) - 1n;

function parseNearAmount(amount, { allowZero = true } = {}) {
  // Numbers may already have been rounded by JSON.parse or the caller.
  const match = typeof amount === 'string' && amount.match(/^([0-9]+)(?:\.([0-9]{1,24}))?$/);
  if (!match || match[0] !== amount) {
    throw new Error('Enter a NEAR amount as a decimal string with up to 24 decimal places.');
  }
  const yocto = BigInt(match[1]) * YOCTO_PER_NEAR + BigInt((match[2] || '').padEnd(24, '0'));
  if (yocto > MAX_YOCTO) throw new Error('NEAR amount exceeds the supported maximum.');
  if (!allowZero && yocto === 0n) throw new Error('NEAR amount must be greater than zero.');
  return yocto;
}

export function validateNearAmount(amount, options) {
  parseNearAmount(amount, options);
  return amount;
}

export function yoctoToNear(yocto) {
  const raw = typeof yocto === 'bigint' ? yocto.toString() : (yocto ?? '0');
  const match = typeof raw === 'string' && raw.match(/^[0-9]+$/);
  if (!match || match[0] !== raw || BigInt(raw) > MAX_YOCTO) {
    throw new Error('Invalid yoctoNEAR amount.');
  }
  const value = BigInt(raw);
  const whole = (value / YOCTO_PER_NEAR).toString();
  const fraction = (value % YOCTO_PER_NEAR).toString().padStart(24, '0').replace(/0+$/, '');
  return fraction ? `${whole}.${fraction}` : whole;
}

export function nearToYocto(near, options) {
  return parseNearAmount(near, options).toString();
}

export function formatNearAmount(amount) {
  const [whole, fraction] = yoctoToNear(amount).split('.');
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return fraction ? `${grouped}.${fraction}` : grouped;
}

export function shortenAccountId(accountId, chars = 6) {
  if (!accountId) return '';
  if (accountId.length <= chars * 2) return accountId;
  return `${accountId.slice(0, chars)}...${accountId.slice(-chars)}`;
}
