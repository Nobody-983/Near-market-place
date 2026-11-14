-- ==============================================================================
-- scripts/reconcile-catalog.sql
-- RoundFrog V2 — Catalog Reconciliation Plan
-- Prepares catalog registration for verified on-chain assets:
-- 1. Repairs the two existing Atlas records with verified token_id, contract_id, and onchain_status.
-- 2. Registers the 10 genuine RoundFrog Genesis Collection NFTs with stable canonical UUIDs.
-- 3. Sets matching nft_ownership and marketplace_listings for verified active listings.
-- 4. Excludes acceptance and QA tokens.
--
-- NOTE: In accordance with safety rules, this script is PREPARED for review.
-- DO NOT execute until explicitly approved by the user.
-- ==============================================================================

BEGIN;

-- ------------------------------------------------------------------------------
-- 1. REPAIR EXISTING ATLAS RECORDS
-- ------------------------------------------------------------------------------

-- Record 1: Atlas owned by dragon983.testnet
UPDATE public.nfts
SET
  token_id = 'catalog-687855bc-bc40-4bae-a299-758a449acb8e',
  nft_contract_id = 'nft-v2.roundfrog-dev.testnet',
  onchain_status = 'confirmed',
  updated_at = NOW()
WHERE id = '687855bc-bc40-4bae-a299-758a449acb8e';

-- Ensure marketplace listing for Atlas (actively listed on-chain at 5 NEAR)
INSERT INTO public.marketplace_listings (
  id,
  nft_id,
  seller_id,
  price,
  currency,
  status,
  network_id,
  marketplace_contract_id,
  contract_sale_id,
  created_at,
  updated_at
)
VALUES (
  '687855bc-bc40-4bae-a299-758a449acb8e',
  '687855bc-bc40-4bae-a299-758a449acb8e',
  '076eec8a-64fc-487a-aeb6-209e12c8f6b1', -- dragon983.testnet profile ID
  5.0,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-687855bc-bc40-4bae-a299-758a449acb8e|dragon983.testnet|1',
  NOW(),
  NOW()
)
ON CONFLICT (id) DO UPDATE
SET
  status = 'active',
  price = 5.0,
  marketplace_contract_id = 'market-v2.roundfrog-dev.testnet',
  contract_sale_id = 'nft-v2.roundfrog-dev.testnet|catalog-687855bc-bc40-4bae-a299-758a449acb8e|dragon983.testnet|1',
  updated_at = NOW();

-- Record 2: Atlas owned by roundfrog-dev.testnet (UNLISTED)
UPDATE public.nfts
SET
  token_id = 'catalog-97d8f220-d520-49e6-bfe8-0528a7711477',
  nft_contract_id = 'nft-v2.roundfrog-dev.testnet',
  onchain_status = 'confirmed',
  updated_at = NOW()
WHERE id = '97d8f220-d520-49e6-bfe8-0528a7711477';

-- ------------------------------------------------------------------------------
-- 2. REGISTER THE 10 ROUNDFROG GENESIS COLLECTION NFTS
-- ------------------------------------------------------------------------------

-- RoundFrog creator/seller profile: 7aa018cc-aeae-41fa-b89a-f119fb4628f4 (roundfrog-dev.testnet)
-- Dragon983 profile: 076eec8a-64fc-487a-aeb6-209e12c8f6b1 (dragon983.testnet)

-- 01. CyberFrog Alpha #01 (Active Listing: 0.5 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000001-0000-4000-8000-000000000001',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'CyberFrog Alpha #01',
  'RoundFrog Genesis Collection #01 — Vanguard Operative equipped with neon visor and plasma core.',
  '/collection/frog-genesis-01.svg',
  'RoundFrog Genesis',
  0.5,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-01',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000001-0000-4000-8000-000000000001', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000001-0000-4000-8000-000000000001',
  'a0000001-0000-4000-8000-000000000001',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  0.5,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-01|roundfrog-dev.testnet|2'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 0.5;

-- 02. Quantum Ribbit #02 (UNLISTED, Owned by dragon983.testnet)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000002-0000-4000-8000-000000000002',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Quantum Ribbit #02',
  'RoundFrog Genesis Collection #02 — Chrono Navigator warping timelines with quantum goggles.',
  '/collection/frog-genesis-02.svg',
  'RoundFrog Genesis',
  0.75,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-02',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000002-0000-4000-8000-000000000002', '076eec8a-64fc-487a-aeb6-209e12c8f6b1')
ON CONFLICT (nft_id) DO UPDATE SET owner_id = '076eec8a-64fc-487a-aeb6-209e12c8f6b1';

-- 03. Solaris Toad #03 (Active Listing: 1.0 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000003-0000-4000-8000-000000000003',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Solaris Toad #03',
  'RoundFrog Genesis Collection #03 — Solar Flare Sentinel channeling photon shield and golden halo.',
  '/collection/frog-genesis-03.svg',
  'RoundFrog Genesis',
  1.0,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-03',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000003-0000-4000-8000-000000000003', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000003-0000-4000-8000-000000000003',
  'a0000003-0000-4000-8000-000000000003',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  1.0,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-03|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 1.0;

-- 04. Vortex Hopper #04 (Active Listing: 1.25 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000004-0000-4000-8000-000000000004',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Vortex Hopper #04',
  'RoundFrog Genesis Collection #04 — Void Stalker phasing through dark matter with spiral optics.',
  '/collection/frog-genesis-04.svg',
  'RoundFrog Genesis',
  1.25,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-04',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000004-0000-4000-8000-000000000004', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000004-0000-4000-8000-000000000004',
  'a0000004-0000-4000-8000-000000000004',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  1.25,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-04|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 1.25;

-- 05. Circuit Croaker #05 (Active Listing: 1.5 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000005-0000-4000-8000-000000000005',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Circuit Croaker #05',
  'RoundFrog Genesis Collection #05 — Synthetic Architect with bio-digital neural circuit implants.',
  '/collection/frog-genesis-05.svg',
  'RoundFrog Genesis',
  1.5,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-05',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000005-0000-4000-8000-000000000005', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000005-0000-4000-8000-000000000005',
  'a0000005-0000-4000-8000-000000000005',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  1.5,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-05|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 1.5;

-- 06. Aurora Leap #06 (Active Listing: 1.8 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000006-0000-4000-8000-000000000006',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Aurora Leap #06',
  'RoundFrog Genesis Collection #06 — Celestial Mystic bearing the Borealis Shimmer and crown of stars.',
  '/collection/frog-genesis-06.svg',
  'RoundFrog Genesis',
  1.8,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-06',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000006-0000-4000-8000-000000000006', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000006-0000-4000-8000-000000000006',
  'a0000006-0000-4000-8000-000000000006',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  1.8,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-06|roundfrog-dev.testnet|2'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 1.8;

-- 07. Neon Sentinel #07 (Active Listing: 2.2 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000007-0000-4000-8000-000000000007',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Neon Sentinel #07',
  'RoundFrog Genesis Collection #07 — Defense Commander clad in titanium hex armor and cyber visor.',
  '/collection/frog-genesis-07.svg',
  'RoundFrog Genesis',
  2.2,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-07',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000007-0000-4000-8000-000000000007', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000007-0000-4000-8000-000000000007',
  'a0000007-0000-4000-8000-000000000007',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  2.2,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-07|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 2.2;

-- 08. Prism Amphibian #08 (Active Listing: 2.5 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000008-0000-4000-8000-000000000008',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Prism Amphibian #08',
  'RoundFrog Genesis Collection #08 — Quantum Alchemist manipulating chromatic refraction fields.',
  '/collection/frog-genesis-08.svg',
  'RoundFrog Genesis',
  2.5,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-08',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000008-0000-4000-8000-000000000008', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000008-0000-4000-8000-000000000008',
  'a0000008-0000-4000-8000-000000000008',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  2.5,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-08|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 2.5;

-- 09. Zenith Bullfrog #09 (Active Listing: 3.0 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000009-0000-4000-8000-000000000009',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Zenith Bullfrog #09',
  'RoundFrog Genesis Collection #09 — Apex Elder meditating on singularity energy and cosmic runes.',
  '/collection/frog-genesis-09.svg',
  'RoundFrog Genesis',
  3.0,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-09',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000009-0000-4000-8000-000000000009', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000009-0000-4000-8000-000000000009',
  'a0000009-0000-4000-8000-000000000009',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  3.0,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-09|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 3.0;

-- 10. Phantom Monarch #10 (Active Listing: 4.5 NEAR)
INSERT INTO public.nfts (
  id, creator_id, name, description, image_url, category, price, currency, source, network_id, token_id, nft_contract_id, onchain_status
) VALUES (
  'a0000010-0000-4000-8000-000000000010',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  'Phantom Monarch #10',
  'RoundFrog Genesis Collection #10 — Genesis Sovereign of RoundFrog realm, crowned in celestial fire.',
  '/collection/frog-genesis-10.svg',
  'RoundFrog Genesis',
  4.5,
  'NEAR',
  'user',
  'testnet',
  'catalog-roundfrog-genesis-10',
  'nft-v2.roundfrog-dev.testnet',
  'confirmed'
) ON CONFLICT (id) DO NOTHING;

INSERT INTO public.nft_ownership (nft_id, owner_id)
VALUES ('a0000010-0000-4000-8000-000000000010', '7aa018cc-aeae-41fa-b89a-f119fb4628f4')
ON CONFLICT (nft_id) DO NOTHING;

INSERT INTO public.marketplace_listings (
  id, nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id, contract_sale_id
) VALUES (
  'a0000010-0000-4000-8000-000000000010',
  'a0000010-0000-4000-8000-000000000010',
  '7aa018cc-aeae-41fa-b89a-f119fb4628f4',
  4.5,
  'NEAR',
  'active',
  'testnet',
  'market-v2.roundfrog-dev.testnet',
  'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-10|roundfrog-dev.testnet|1'
) ON CONFLICT (id) DO UPDATE SET status = 'active', price = 4.5;

COMMIT;

