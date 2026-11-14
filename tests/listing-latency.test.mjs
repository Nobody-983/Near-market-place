import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import vm from 'node:vm';
import * as amounts from '../frontend/Nft/src/lib/near/utils.js';

const quietConsole = { error() {}, warn() {}, log() {} };

async function loadModule(path, imports) {
  const context = vm.createContext({ console: quietConsole, Buffer, setTimeout, clearTimeout, Date, performance });
  const source = await readFile(new URL(path, import.meta.url), 'utf8');
  const module = new vm.SourceTextModule(source, { context });
  await module.link((name) => {
    assert.ok(imports[name], `Unexpected import: ${name}`);
    return new vm.SyntheticModule(Object.keys(imports[name]), function () {
      for (const [key, value] of Object.entries(imports[name])) this.setExport(key, value);
    }, { context });
  });
  await module.evaluate();
  return module.namespace;
}

const contract = await loadModule('../frontend/Nft/src/lib/near/contract.js', {
  './utils': amounts,
  '../../config/near': {
    NETWORK_ID: 'testnet',
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    MARKETPLACE_CONTRACT_ID: 'market.roundfrog-dev.testnet',
    REVIEWED_NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    REVIEWED_MARKETPLACE_CONTRACT_ID: 'market.roundfrog-dev.testnet',
    REVIEWED_NFT_CODE_HASH: 'nft-reviewed-hash',
    REVIEWED_MARKETPLACE_CODE_HASH: 'market-reviewed-hash',
  },
  '../../config/marketplace': { LISTING_STORAGE_TARGET_YOCTO: '120000000000000000000000' },
});

const { waitForFinal, assertFinalSuccess } = contract;

test('waitForFinal returns immediately (<5ms) when wallet outcome is already finalized object', async () => {
  const finalizedOutcome = {
    transaction: { hash: '8z8j1gWz6rR8bQYx1wJv7aYx4g1wz2a4v5g6h7j8k9L' },
    status: { SuccessValue: '' },
    receipts_outcome: [
      {
        id: 'receipt-1',
        outcome: {
          status: { SuccessValue: '' },
          logs: [],
        },
      },
    ],
  };

  const start = performance.now();
  const result = await waitForFinal(finalizedOutcome, 'alice.testnet', null);
  const elapsed = performance.now() - start;

  assert.equal(result.transaction.hash, '8z8j1gWz6rR8bQYx1wJv7aYx4g1wz2a4v5g6h7j8k9L');
  assert.ok(result.status.SuccessValue !== undefined);
  assert.ok(elapsed < 5, `Expected immediate return (<5ms), got ${elapsed.toFixed(2)}ms`);
});

test('waitForFinal returns immediately (<5ms) when wallet outcome is an array of outcomes', async () => {
  const finalizedOutcome = {
    transaction: { hash: '8z8j1gWz6rR8bQYx1wJv7aYx4g1wz2a4v5g6h7j8k9L' },
    status: { SuccessValue: 'AQ==' },
    receipts_outcome: [],
  };

  const start = performance.now();
  const result = await waitForFinal([finalizedOutcome], 'bob.testnet', null);
  const elapsed = performance.now() - start;

  assert.equal(result.transaction.hash, '8z8j1gWz6rR8bQYx1wJv7aYx4g1wz2a4v5g6h7j8k9L');
  assert.ok(result.status.SuccessValue !== undefined);
  assert.ok(elapsed < 5, `Expected immediate return (<5ms), got ${elapsed.toFixed(2)}ms`);
});

test('waitForFinal starts progressive polling at 400ms when given a valid transaction hash', async () => {
  const validBase58Hash = '9FG8HwN3sZz1W17xXkL9J8v2mNpQrStUvWxYz123456';
  const mockCalls = [];
  const fakeOutcome = {
    transaction: { hash: validBase58Hash },
    status: { SuccessValue: '' },
    receipts_outcome: [],
  };

  let attempts = 0;
  const mockProvider = {
    async txStatus(hash, accountId, finality) {
      mockCalls.push({ hash, accountId, finality, time: performance.now() });
      attempts++;
      if (attempts === 1) {
        throw new Error('Not found yet in block');
      }
      return fakeOutcome;
    },
  };

  const customWallet = {
    provider: mockProvider,
  };

  const progressUpdates = [];
  const start = performance.now();
  const result = await waitForFinal(validBase58Hash, 'carol.testnet', customWallet, 5000, (step) => {
    progressUpdates.push(step);
  });
  const elapsed = performance.now() - start;

  assert.equal(result.transaction.hash, validBase58Hash);
  assert.equal(attempts, 2);
  assert.ok(progressUpdates.includes('Processing on NEAR...'));
  // Two attempts with first 400ms delay: total time should be between 380ms and 850ms
  // Previously with 1000ms delay this would take >1000ms.
  assert.ok(elapsed >= 380 && elapsed < 850, `Expected progressive polling (~400ms delay), elapsed: ${elapsed.toFixed(2)}ms`);
});

test('waitForFinal immediately returns failure outcome without retrying or hanging', async () => {
  const failureOutcome = {
    transaction: { hash: '8z8j1gWz6rR8bQYx1wJv7aYx4g1wz2a4v5g6h7j8k9L' },
    status: {
      Failure: {
        ActionError: {
          kind: { FunctionCallError: { ExecutionError: 'Smart contract panicked' } },
        },
      },
    },
  };

  const start = performance.now();
  const result = await waitForFinal(failureOutcome, 'alice.testnet', null);
  const elapsed = performance.now() - start;

  assert.ok(result.status.Failure !== undefined);
  assert.ok(elapsed < 5, `Expected immediate return on failure (<5ms), got ${elapsed.toFixed(2)}ms`);

  // assertFinalSuccess must strictly throw on this outcome
  assert.throws(
    () => assertFinalSuccess(result, 'Test Action'),
    /failed on NEAR/
  );
});

test('assertFinalSuccess verifies receipt outcomes and rejects hidden receipt errors', () => {
  const receiptFailedOutcome = {
    transaction: { hash: '8z8j1gWz6rR8bQYx1wJv7aYx4g1wz2a4v5g6h7j8k9L' },
    status: { SuccessValue: '' },
    receipts_outcome: [
      {
        id: 'receipt-1',
        outcome: {
          status: {
            Failure: {
              ActionError: {
                kind: { FunctionCallError: { ExecutionError: 'Storage deposit required' } },
              },
            },
          },
          logs: [],
        },
      },
    ],
  };

  assert.throws(
    () => assertFinalSuccess(receiptFailedOutcome, 'Listing approval'),
    /failed in a NEAR receipt/
  );
});
