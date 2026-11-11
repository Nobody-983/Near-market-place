// Minimal server-only PostgREST client for the two settlement operations.
// Keeping it here avoids an undeclared runtime dependency in this service.
class QueryBuilder {
  constructor(client, table) {
    this.client = client;
    this.table = table;
    this.params = new URLSearchParams();
  }
  select(columns) { this.params.set('select', columns); return this; }
  eq(column, value) { this.params.append(column, `eq.${value}`); return this; }
  limit(value) { this.params.set('limit', String(value)); return this.client.request(`/${this.table}?${this.params}`); }
}

class SettlementSupabaseClient {
  constructor(url, serviceRoleKey) {
    const endpoint = new URL(url);
    if (endpoint.protocol !== 'https:') throw new Error('SUPABASE_URL must use HTTPS');
    this.url = new URL('/rest/v1', endpoint).toString().replace(/\/$/, '');
    this.serviceRoleKey = serviceRoleKey;
  }
  from(table) {
    if (table !== 'marketplace_listings') throw new Error('Unexpected settlement table');
    return new QueryBuilder(this, table);
  }
  rpc(name, params) {
    if (!new Set([
      'reconcile_nft_purchase', 'bind_verified_nft_mint',
      'publish_verified_listing', 'cancel_verified_listing',
      'record_withdrawal_intent', 'confirm_withdrawal', 'fail_withdrawal',
    ]).has(name)) throw new Error('Unexpected trusted RPC');
    return this.request(`/rpc/${name}`, { method: 'POST', body: JSON.stringify(params) });
  }
  async request(path, init = {}) {
    try {
      const response = await fetch(`${this.url}${path}`, {
        ...init,
        signal: AbortSignal.timeout(20_000),
        headers: {
          apikey: this.serviceRoleKey,
          Authorization: `Bearer ${this.serviceRoleKey}`,
          Accept: 'application/json',
          'Content-Type': 'application/json',
          ...(init.headers || {}),
        },
      });
      const text = await response.text();
      let data = null;
      if (text) {
        try { data = JSON.parse(text); } catch { return { data: null, error: { message: 'Invalid Supabase JSON' } }; }
      }
      if (!response.ok) {
        return { data: null, error: { message: data?.message || `Supabase HTTP ${response.status}` } };
      }
      return { data, error: null };
    } catch (error) {
      return { data: null, error: { message: error.message } };
    }
  }
}

module.exports = { SettlementSupabaseClient };
