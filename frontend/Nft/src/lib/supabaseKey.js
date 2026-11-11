// Browser bundles may only carry the public/publishable Supabase key.
export function assertPublishableKey(key) {
  if (typeof key !== "string" || key.length === 0) {
    throw new Error("A Supabase publishable (anon) key is required.");
  }
  if (key.startsWith("sb_secret_") || key.startsWith("service_role")) {
    throw new Error("Supabase secret/service-role keys must never be exposed to Vite.");
  }
  const parts = key.split(".");
  if (parts.length === 3) {
    let payload;
    try {
      payload = JSON.parse(
        globalThis.atob(parts[1].replace(/-/g, "+").replace(/_/g, "/")),
      );
    } catch {
      payload = null;
    }
    if (payload?.role === "service_role") {
      throw new Error("Supabase secret/service-role keys must never be exposed to Vite.");
    }
  }
  return key;
}
