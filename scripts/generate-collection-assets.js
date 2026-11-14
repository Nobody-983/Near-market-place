import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const outDir = path.resolve(__dirname, "../frontend/Nft/public/collection");

if (!fs.existsSync(outDir)) {
  fs.mkdirSync(outDir, { recursive: true });
}

const nfts = [
  {
    id: "frog-genesis-01",
    name: "CyberFrog Alpha #01",
    role: "Vanguard Operative",
    palette: { bg1: "#0b0c16", bg2: "#191735", primary: "#9A8AFB", secondary: "#62C9B8", accent: "#ff007a" },
    eyeGlow: "#62C9B8",
    visor: true,
    pattern: "grid",
    rarity: "Epic",
  },
  {
    id: "frog-genesis-02",
    name: "Quantum Ribbit #02",
    role: "Chrono Navigator",
    palette: { bg1: "#0a131a", bg2: "#112638", primary: "#62C9B8", secondary: "#9A8AFB", accent: "#00f0ff" },
    eyeGlow: "#9A8AFB",
    visor: false,
    goggles: true,
    pattern: "quantum",
    rarity: "Rare",
  },
  {
    id: "frog-genesis-03",
    name: "Solaris Toad #03",
    role: "Solar Flare Sentinel",
    palette: { bg1: "#150e0c", bg2: "#2d1612", primary: "#E7B66D", secondary: "#9A8AFB", accent: "#ff5e36" },
    eyeGlow: "#E7B66D",
    visor: true,
    pattern: "solar",
    rarity: "Rare",
  },
  {
    id: "frog-genesis-04",
    name: "Vortex Hopper #04",
    role: "Void Stalker",
    palette: { bg1: "#0a0715", bg2: "#1b0f38", primary: "#9A8AFB", secondary: "#ff3df1", accent: "#62C9B8" },
    eyeGlow: "#ff3df1",
    visor: false,
    spiral: true,
    pattern: "vortex",
    rarity: "Epic",
  },
  {
    id: "frog-genesis-05",
    name: "Circuit Croaker #05",
    role: "Synthetic Architect",
    palette: { bg1: "#081412", bg2: "#0f2b25", primary: "#62C9B8", secondary: "#25845D", accent: "#9A8AFB" },
    eyeGlow: "#62C9B8",
    visor: true,
    circuit: true,
    pattern: "circuits",
    rarity: "Uncommon",
  },
  {
    id: "frog-genesis-06",
    name: "Aurora Leap #06",
    role: "Celestial Mystic",
    palette: { bg1: "#08101e", bg2: "#142544", primary: "#65C99A", secondary: "#9A8AFB", accent: "#62C9B8" },
    eyeGlow: "#65C99A",
    crown: true,
    pattern: "aurora",
    rarity: "Legendary",
  },
  {
    id: "frog-genesis-07",
    name: "Neon Sentinel #07",
    role: "Defense Commander",
    palette: { bg1: "#10101b", bg2: "#1e1d35", primary: "#6957D9", secondary: "#62C9B8", accent: "#38bdf8" },
    eyeGlow: "#38bdf8",
    visor: true,
    armor: true,
    pattern: "hex",
    rarity: "Epic",
  },
  {
    id: "frog-genesis-08",
    name: "Prism Amphibian #08",
    role: "Quantum Alchemist",
    palette: { bg1: "#13091e", bg2: "#2d1245", primary: "#9A8AFB", secondary: "#f472b6", accent: "#62C9B8" },
    eyeGlow: "#9A8AFB",
    horns: true,
    pattern: "prism",
    rarity: "Legendary",
  },
  {
    id: "frog-genesis-09",
    name: "Zenith Bullfrog #09",
    role: "Apex Elder",
    palette: { bg1: "#0d1512", bg2: "#1a3028", primary: "#268E83", secondary: "#E7B66D", accent: "#9A8AFB" },
    eyeGlow: "#E7B66D",
    orb: true,
    pattern: "cosmic",
    rarity: "Mythic",
  },
  {
    id: "frog-genesis-10",
    name: "Phantom Monarch #10",
    role: "Genesis Sovereign",
    palette: { bg1: "#100a1c", bg2: "#281347", primary: "#9A8AFB", secondary: "#E7B66D", accent: "#62C9B8" },
    eyeGlow: "#E7B66D",
    crown: true,
    scepter: true,
    pattern: "royalty",
    rarity: "Mythic",
  },
];

for (const nft of nfts) {
  const p = nft.palette;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 600" width="100%" height="100%">
  <defs>
    <radialGradient id="bgGrad" cx="50%" cy="40%" r="75%">
      <stop offset="0%" stop-color="${p.bg2}"/>
      <stop offset="100%" stop-color="${p.bg1}"/>
    </radialGradient>
    <linearGradient id="bodyGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${p.primary}"/>
      <stop offset="100%" stop-color="${p.secondary}"/>
    </linearGradient>
    <linearGradient id="accentGrad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="${p.primary}"/>
      <stop offset="50%" stop-color="${p.accent}"/>
      <stop offset="100%" stop-color="${p.secondary}"/>
    </linearGradient>
    <filter id="neonGlow" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="8" result="coloredBlur"/>
      <feMerge>
        <feMergeNode in="coloredBlur"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
    <filter id="softGlow" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="16" result="blur"/>
      <feMerge>
        <feMergeNode in="blur"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
  </defs>

  <!-- Background Canvas -->
  <rect width="600" height="600" fill="url(#bgGrad)" />

  <!-- Ambient background glow aura -->
  <circle cx="300" cy="300" r="180" fill="${p.primary}" opacity="0.15" filter="url(#softGlow)" />
  <circle cx="300" cy="270" r="120" fill="${p.accent}" opacity="0.12" filter="url(#softGlow)" />

  <!-- Background Geo Framework -->
  <g stroke="${p.secondary}" stroke-width="1" opacity="0.25">
    <circle cx="300" cy="300" r="240" fill="none" stroke-dasharray="6,6" />
    <circle cx="300" cy="300" r="270" fill="none" stroke-dasharray="2,8" />
    <line x1="60" y1="300" x2="540" y2="300" />
    <line x1="300" y1="60" x2="300" y2="540" />
    <!-- Diagonal tick marks -->
    <path d="M 120 120 L 140 140 M 480 120 L 460 140 M 120 480 L 140 460 M 480 480 L 460 460" stroke-width="2" />
  </g>

  <!-- Cybernetic Floating Emblems / Rarity Frame -->
  <rect x="25" y="25" width="550" height="550" rx="20" fill="none" stroke="url(#accentGrad)" stroke-width="2" opacity="0.6"/>
  <rect x="35" y="35" width="530" height="530" rx="14" fill="none" stroke="${p.primary}" stroke-width="1" opacity="0.3" stroke-dasharray="10, 15"/>

  <!-- Frog Body Base Silhouette -->
  <!-- Torso & Shoulders -->
  <path d="M 210 440 C 190 380 180 340 230 310 C 260 290 340 290 370 310 C 420 340 410 380 390 440 C 370 480 230 480 210 440 Z" fill="url(#bodyGrad)" opacity="0.85" />

  <!-- Cyber Chest Core / Emblem -->
  <polygon points="300,340 330,370 300,410 270,370" fill="${p.bg1}" stroke="${p.accent}" stroke-width="3" filter="url(#neonGlow)"/>
  <circle cx="300" cy="373" r="8" fill="${p.eyeGlow}" filter="url(#neonGlow)"/>

  <!-- Frog Head & Iconic Cheeks -->
  <ellipse cx="300" cy="270" rx="115" ry="85" fill="url(#bodyGrad)" stroke="${p.primary}" stroke-width="2"/>
  <path d="M 200 275 C 200 320 250 335 300 335 C 350 335 400 320 400 275 C 400 240 370 215 300 215 C 230 215 200 240 200 275 Z" fill="${p.bg1}" opacity="0.4" />

  <!-- Iconic Frog Eye Pods -->
  <!-- Left Eye Pod -->
  <circle cx="230" cy="205" r="46" fill="url(#bodyGrad)" stroke="${p.primary}" stroke-width="3" />
  <circle cx="230" cy="205" r="34" fill="${p.bg1}" stroke="${p.accent}" stroke-width="2" />
  <circle cx="230" cy="205" r="22" fill="${nft.eyeGlow}" filter="url(#neonGlow)" />
  <ellipse cx="230" cy="205" rx="6" ry="18" fill="#FFFFFF" />

  <!-- Right Eye Pod -->
  <circle cx="370" cy="205" r="46" fill="url(#bodyGrad)" stroke="${p.primary}" stroke-width="3" />
  <circle cx="370" cy="205" r="34" fill="${p.bg1}" stroke="${p.accent}" stroke-width="2" />
  <circle cx="370" cy="205" r="22" fill="${nft.eyeGlow}" filter="url(#neonGlow)" />
  <ellipse cx="370" cy="205" rx="6" ry="18" fill="#FFFFFF" />

  <!-- Special Trait Overlays -->
  ${nft.visor ? `
  <!-- Cyber HUD Visor -->
  <path d="M 195 210 Q 300 235 405 210 L 395 240 Q 300 265 205 240 Z" fill="${p.accent}" opacity="0.85" filter="url(#neonGlow)"/>
  <line x1="210" y1="225" x2="390" y2="225" stroke="#FFFFFF" stroke-width="1.5" stroke-dasharray="4,4"/>
  ` : ""}

  ${nft.crown ? `
  <!-- Sovereign Crown of Starlight -->
  <polygon points="250,155 270,120 300,145 330,120 350,155 300,165" fill="${p.accent}" stroke="#FFFFFF" stroke-width="2" filter="url(#neonGlow)"/>
  <circle cx="270" cy="115" r="4" fill="#FFFFFF" filter="url(#neonGlow)"/>
  <circle cx="300" cy="110" r="6" fill="#FFFFFF" filter="url(#neonGlow)"/>
  <circle cx="330" cy="115" r="4" fill="#FFFFFF" filter="url(#neonGlow)"/>
  ` : ""}

  ${nft.circuit ? `
  <!-- Bio-digital Circuit Traces -->
  <g stroke="${p.accent}" stroke-width="2" fill="none" opacity="0.85" filter="url(#neonGlow)">
    <path d="M 240 280 L 260 300 L 260 325" />
    <circle cx="260" cy="325" r="3" fill="${p.accent}" />
    <path d="M 360 280 L 340 300 L 340 325" />
    <circle cx="340" cy="325" r="3" fill="${p.accent}" />
    <path d="M 300 240 L 300 270" />
  </g>
  ` : ""}

  <!-- Mouth / Cybernetic Grill -->
  <path d="M 265 295 Q 300 310 335 295" stroke="${p.primary}" stroke-width="4" stroke-linecap="round" fill="none" />
  <line x1="285" y1="298" x2="285" y2="304" stroke="${p.secondary}" stroke-width="2"/>
  <line x1="300" y1="300" x2="300" y2="306" stroke="${p.secondary}" stroke-width="2"/>
  <line x1="315" y1="298" x2="315" y2="304" stroke="${p.secondary}" stroke-width="2"/>

  <!-- Badge Details Banner -->
  <g transform="translate(300, 520)">
    <!-- Label Backdrop -->
    <rect x="-140" y="-18" width="280" height="36" rx="10" fill="${p.bg1}" stroke="url(#accentGrad)" stroke-width="1.5" />
    <text x="0" y="5" font-family="system-ui, -apple-system, sans-serif" font-size="14" font-weight="800" fill="#FFFFFF" text-anchor="middle" letter-spacing="2">
      ${nft.name.toUpperCase()}
    </text>
  </g>

  <!-- Tag Pill -->
  <g transform="translate(300, 75)">
    <rect x="-90" y="-14" width="180" height="28" rx="14" fill="${p.bg1}" stroke="${p.primary}" stroke-width="1" opacity="0.85" />
    <text x="0" y="4" font-family="system-ui, -apple-system, sans-serif" font-size="11" font-weight="600" fill="${p.primary}" text-anchor="middle" letter-spacing="1.5">
      ROUNDFROG • ${nft.rarity.toUpperCase()}
    </text>
  </g>
</svg>`;

  const filePath = path.join(outDir, `${nft.id}.svg`);
  fs.writeFileSync(filePath, svg.trim());
  console.log(`[+] Wrote ${filePath}`);
}

console.log("[✓] Successfully generated 10 original RoundFrog Genesis SVG artworks!");

