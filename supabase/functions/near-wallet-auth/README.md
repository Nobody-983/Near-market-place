# near-wallet-auth Edge Function

NEP-413 wallet authentication for RoundFrog (Supabase Edge Function).

## Required server secrets/config (set via `supabase secrets set`, never in frontend)
- `NEAR_AUTH_APP_ORIGIN` (**explicitly required**) – exact browser origin (e.g. `https://app.example.com`; `http://localhost:5173` only for local dev). Must equal the `Origin` header the browser sends. **Do not fabricate.** The handler rejects invalid/missing origin config at startup (common cause of HTTP 500 `WORKER_ERROR`).
- `NEAR_AUTH_NETWORK_ID` – `testnet` or `mainnet` (secret name present for project `llqwiqhwrbfoxoqqgczy`; value must be `testnet` for this deployment).
- `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY` are normally provided/injected by the Supabase Edge runtime; verify they are available in project settings. The function requires an HTTPS project URL and a service-role key server-side only.

## Deploy
```sh
npx supabase functions deploy near-wallet-auth --project-ref llqwiqhwrbfoxoqqgczy --no-verify-jwt --yes
```
`--no-verify-jwt` is required because `challenge` (login phase) occurs before a Supabase session exists; the function enforces origin, challenge binding, NEP-413 signature verification, NEAR FullAccess key checks, replay/expiry protection, and session-bound authorization itself.

## Safe validation (no wallet signature)
- **Allowed origin challenge (login):** POST JSON `{"action":"challenge","networkId":"testnet","accountId":"<valid.testnet>","phase":"login","requestHash":"<64 hex>"}` with matching `Origin` and publishable `apikey`/`Authorization`. Expect **HTTP 200** with `{ challenge: ... }` once secrets/config are correct.
- **Invalid origin:** expect **403** `Origin not allowed` (or config/startup failure if origin misconfigured).
- **Malformed request:** wrong method/content-type/action/network/hash → 4xx with safe error.
- **Replay/expiry:** consumed/expired challenge verification rejected by server/DB.
- **Session actions (`status`/`logout`/session verify):** require valid Supabase bearer session; do not test with a real wallet without explicit approval.
- No service-role key, admin link, hashed token, or OTP is returned.

## Security notes
- Service-role operations stay server-side. No `VITE_*` secret.
- Login challenges are rate-limited; session challenges require a valid Supabase session and wallet/session binding.
- FullAccess key authority is checked against NEAR RPC (final).
