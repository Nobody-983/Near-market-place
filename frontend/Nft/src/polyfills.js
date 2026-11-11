import { Buffer } from 'buffer';

if (typeof window !== 'undefined') {
  window.global = window;
  window.Buffer = Buffer;
  if (!window.process) {
    window.process = {
      env: {},
      nextTick: (fn, ...args) => setTimeout(() => fn(...args), 0),
      browser: true,
      version: '',
    };
  }
}

if (typeof globalThis !== 'undefined') {
  globalThis.global = globalThis;
  globalThis.Buffer = Buffer;
  if (!globalThis.process && typeof window !== 'undefined') {
    globalThis.process = window.process;
  }
}
