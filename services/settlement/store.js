// Durable scan checkpoint plus an advisory audit ledger. Supabase remains the
// authoritative idempotency boundary, so a local entry never skips SQL.

const fs = require('fs');
const path = require('path');

class ProcessedStore {
  constructor(filePath) {
    this.filePath = filePath;
    this.set = new Set();
    this.cursor = null;
    this.load();
  }

  load() {
    try {
      if (fs.existsSync(this.filePath)) {
        const lines = fs.readFileSync(this.filePath, 'utf8')
          .split('\n')
          .map((s) => s.trim())
          .filter(Boolean);
        for (const line of lines) {
          if (line.startsWith('cursor\t')) {
            const [, rawHeight, hash] = line.split('\t');
            const height = Number(rawHeight);
            if (Number.isSafeInteger(height) && height >= 0 && hash) this.cursor = { height, hash };
          } else if (line.startsWith('processed\t')) {
            this.set.add(line.slice('processed\t'.length));
          } else {
            // Legacy files stored one transaction hash per line.
            this.set.add(line);
          }
        }
      }
    } catch (err) {
      throw new Error(`ProcessedStore load failed: ${err.message}`);
    }
  }

  has(id) {
    return this.set.has(id);
  }

  add(id) {
    if (this.set.has(id)) return false;
    this.set.add(id);
    this.flush();
    return true;
  }

  getCursor() {
    return this.cursor && { ...this.cursor };
  }

  setCursor(height, hash) {
    if (!Number.isSafeInteger(height) || height < 0 || !hash) {
      throw new Error('Invalid settlement checkpoint');
    }
    this.cursor = { height, hash };
    this.flush();
  }

  flush() {
    fs.mkdirSync(path.dirname(this.filePath), { recursive: true });
    const lines = [];
    if (this.cursor) lines.push(`cursor\t${this.cursor.height}\t${this.cursor.hash}`);
    for (const id of [...this.set].sort()) lines.push(`processed\t${id}`);
    const temporary = `${this.filePath}.${process.pid}.tmp`;
    fs.writeFileSync(temporary, `${lines.join('\n')}\n`, { mode: 0o600 });
    fs.renameSync(temporary, this.filePath);
  }
}

module.exports = { ProcessedStore };
