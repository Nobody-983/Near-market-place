import { createWalletAuthHandler } from './handler.mjs';

// Server-only secrets. No VITE_* credential or frontend service-role key.
Deno.serve(createWalletAuthHandler({
  supabaseUrl: Deno.env.get('SUPABASE_URL'),
  serviceRoleKey: Deno.env.get('SUPABASE_SERVICE_ROLE_KEY'),
  appOrigin: Deno.env.get('NEAR_AUTH_APP_ORIGIN'),
  networkId: Deno.env.get('NEAR_AUTH_NETWORK_ID'),
}));
