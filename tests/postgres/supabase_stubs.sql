-- Minimal disposable PostgreSQL 15 stand-ins for Supabase-managed objects.
-- These exist only so repository migrations can be executed locally; they do
-- not simulate GoTrue, PostgREST, Storage API validation, or hosted Supabase.
create role anon nologin;
create role authenticated nologin;
create role service_role nologin bypassrls;

create schema auth;
create table auth.users (
  id uuid primary key,
  email text,
  raw_app_meta_data jsonb not null default '{}'::jsonb
);
create table auth.sessions (
  id uuid primary key,
  user_id uuid not null references auth.users(id) on delete cascade,
  not_after timestamptz
);
create function auth.jwt() returns jsonb language sql stable as $$
  select coalesce(nullif(current_setting('request.jwt.claims', true), ''), '{}')::jsonb
$$;
create function auth.uid() returns uuid language sql stable as $$
  select nullif(auth.jwt() ->> 'sub', '')::uuid
$$;

create schema storage;
create table storage.buckets (
  id text primary key,
  name text not null unique,
  public boolean not null default false,
  file_size_limit bigint,
  allowed_mime_types text[]
);
create table storage.objects (
  id uuid primary key default gen_random_uuid(),
  bucket_id text not null references storage.buckets(id),
  name text not null,
  owner_id text,
  unique (bucket_id, name)
);
alter table storage.buckets enable row level security;
alter table storage.objects enable row level security;

grant usage on schema auth, storage to anon, authenticated, service_role;
grant select on auth.users, auth.sessions to service_role;
grant select, insert on storage.buckets, storage.objects to service_role;
