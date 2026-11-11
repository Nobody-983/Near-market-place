# Validation case recipes and acceptance evidence

All twenty cases are **NOT_RUN on the pinned node/TestLoop** in the
[2026-11-03 report](reports/validation-20261103/report.json). This table defines
acceptance, not observed behavior. Recipes reside in `cases.py`. Every independent
variant starts a fresh chain and manifest. No contract bytes are instrumented.

Common setup: protocol 88, pinned executable and approved Wasm hashes, isolated
loopback network, three fresh local signers/accounts with local genesis balances,
finite expiry. “Deploy” means one `[DeployContract, FunctionCall(init)]` self-signed
transaction per contract. “Candidate” means deploy, fund, self-mint and approve
with `msg:null`; “Ready” additionally creates the immutable admission.

Common evidence: pre-submission signed journal; raw FINAL transaction/outcomes;
authentic initial, child, refund and data receipt bodies; before/after state,
code, key, nonce, storage and balance snapshots at pinned finalized blocks.
`verify.py` independently checks these transaction bundles. Recipe-specific
assertions additionally inspect the artifacts listed below. Neither a successful
setup nor a passing transaction bundle alone certifies the entire case.

| ID | Setup and action | Expected behavior | Additional evidence / independent acceptance |
| --- | --- | --- | --- |
| S01 | Fresh empty contract accounts; deploy/init both | Exact approved code, bound NFT authority and market admission owner; trading disabled | Signed two-action batches; executing code hashes; NFT views and raw bootstrap/owner envelopes |
| S02 | Fresh account per role; deploy with invalid metadata/domain initializer | Initializer action 1 fails during function execution; code/state roll back | Failure `ActionError.index=1`; identical full before/after application state; nonce and fee effects retained |
| S03 | Fresh account per role; one raw gas unit for initializer | Accepted receipt fails in initializer action 1, not transaction preflight or deployment | Same rollback evidence, function-execution failure phase and gas error; rejected transactions do not qualify |
| S04 | Fresh role account; `[Deploy, Init, Init]` | Repeated initializer action 2 fails; entire batch rolls back | Failure index 2 and function error, absent code/root/sidecars; fees/nonces reconciled |
| S05 | Calibrate actual funding/mint/approval storage; fresh variants at exact cost and cost−1 | Exact minimum succeeds; insufficient storage fails without surviving application changes | Calibration source transaction files; raw byte deltas × byte cost; rollback and complete refunds; gas failures do not count as storage boundaries |
| S06 | Candidate; `creation_bytes=B−1` using calibrated B | Post-write creation budget check fails atomically | Exact failure message, full raw state/credit equality, no new admission/operation/reservation records |
| S07 | Ready admission; dispatch | Two authentic NFT calls and private callback complete Active; cursor 3 | Canonical data-ID ordering, payloads, child gas/deposits, unchanged NFT ownership/approval and zero callback storage growth |
| S08 | Fresh Ready variants with one gas unit for first, second, or both NFT calls | Targeted children fail, callback succeeds Invalid, parent remains committed | Per-child failures, authentic failed-data payloads (`None`), callback outcome and Invalid state |
| S09 | Ready admission; callback gas=1, then adaptive callback sweep | NFT calls succeed; callback OOG leaves Dispatched/cursor 2; retry emits no children; release fails | Failed/passed bracket at 0.1 Tgas resolution, child and callback outcomes, unchanged retry state, rejected release |
| S10 | Fresh variant for each gas point and fund/mint/approve/create/dispatch stage | Success/failure bracket; runtime failures roll back writes/promises | Gas brackets, raw failure phase, unchanged state and no escaped child graph. Internal post-write OOG location remains INCONCLUSIVE without instruction evidence |
| S11 | Calibrate NFT result size N; fresh bounds 5, N−1, N, N+1 | Authentic result accepted at N/N+1 and rejected at lower bounds; successful callback | Exact return bytes, immutable bound, Invalid/Active lifecycle and no callback allocation |
| S12 | Ready admission; finalize approval revocation; dispatch | Authentic stale approval observation yields Invalid | Finalized revoke, unchanged owner, actual NFT approval result, canonical callback graph |
| S13 | Ready admission; replay funding nonce, create, and pending dispatch | Duplicate funding fails/refunds; create/dispatch retry is read-only with no second graph | Credits, nonce-bound journals, full raw state equality and no duplicate child receipts |
| S14 | Ready release; separate dispatched and completed variants attempt release | Only Ready allocation releases unused owner credit; dispatch after release fails | Retained historical reservation, release guard, fixed-size state, original owner attribution; rejected later releases |
| S15 | Actual loopback proxy drops before forwarding or after forwarding funding/dispatch once | Client keeps UNKNOWN and never auto-resubmits; forwarded transaction resolves once | `fault-proxy.json`, immutable journal/hash, FINAL evidence or `unforwarded-observation.json`; bounded non-observation is not permanent absence |
| S16 | Ready admission; dispatch, stop and restart same database | Process restart succeeds; pending-callback continuation requires genuine stopped-queue proof | `restart-checkpoint.json` and final graph. A pre-stop pending snapshot alone cannot certify work was still queued at shutdown; current recipe remains INCONCLUSIVE for that claim |
| S17 | Calibrated exact deposits, +1 and +2 yocto variants; complete lifecycle | Correct retained amounts/refund branch, storage liabilities and aggregate conservation | All protocol/application refunds; unique outcome burns; +2 refund versus +1 retained dust; per-account snapshots retained |
| L01 | Four TestLoop validators, distinct NFT/market shards; delay NFT endorsements after committed parent, resume | Pending Dispatched state before delivery, eventual Active callback | Topology, real withheld-endorsement count, delay gate, pending snapshot and complete graph; no timing claim from stored receipt existence alone |
| L02 | Same controlled delay crosses finite expiry, then resumes | Actual advanced block timestamp causes Invalid callback | Expiry from local manifest, timestamped pending/expired snapshots, resumed authentic callback; block height alone is insufficient |
| L03 | Ready TestLoop candidate; alternate individual data orders/withhold one | Runtime waits for both, canonical result mapping after release | **Missing adapter:** current bridge only declares this capability absent. Baseline success is never PASS for L03 |

The independent verifier currently covers transaction/receipt/state consistency;
full case acceptance also requires reviewing the recipe-specific evidence above.
Build, synthetic-fixture and native-unit-test results must never be substituted
for S/L execution. Reports distinguish unavailable execution (`NOT_RUN`) from
executed but insufficient evidence (`INCONCLUSIVE`) and disproved assertions
(`FAIL`). Public approval flags and trading gates remain separate.
