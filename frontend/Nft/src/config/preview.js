// Preview/demo data is display-only and must never be mistaken for indexed
// marketplace state. It is disabled unless explicitly enabled in local env.
const runtimeEnv = import.meta.env || {};

export const PREVIEW_DATA_ENABLED = runtimeEnv.VITE_PREVIEW_DATA_ENABLED === "true";
