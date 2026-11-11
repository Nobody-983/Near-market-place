import { createClient } from "@supabase/supabase-js";
import { assertPublishableKey } from "./supabaseKey";

const runtimeEnv = import.meta.env || {};

const supabaseUrl =
  runtimeEnv.VITE_SUPABASE_URL || "https://placeholder-project.supabase.co";
const supabasePublishableKey =
  runtimeEnv.VITE_SUPABASE_PUBLISHABLE_KEY || "placeholder-anon-key";

// Only a publishable/anon key may enter the browser bundle. A secret or
// service-role key would bypass RLS, so fail closed instead of shipping it.
export const supabase = createClient(
  supabaseUrl,
  assertPublishableKey(supabasePublishableKey),
);
