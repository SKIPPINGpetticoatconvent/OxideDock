import { invoke } from "@tauri-apps/api/core";

// ═══════════════════════════════════════════════════════════
// OxideDock — macOS Dock Magnification Engine
// ═══════════════════════════════════════════════════════════

interface Shortcut {
  name: string;
  path: string;
}

interface Category {
  name: string;
  shortcuts: Shortcut[];
}

interface Config {
  categories: Category[];
}

// ─── Magnification parameters (macOS-faithful) ───
const BASE_SIZE = 64;         // Icon base size in px
const MAX_SCALE = 1.65;       // Maximum magnification
const MAGNIFY_RANGE = 200;    // Pixels of influence from cursor
const LERP_SPEED = 0.18;      // Smooth interpolation factor
const SPRING_SPEED = 0.14;    // Return-to-rest spring speed

// ─── State ───
let dockBarEl: HTMLElement | null = null;
let dockItems: HTMLElement[] = [];
let mouseX = -9999;
let isHovering = false;
let currentScales: number[] = [];
let targetScales: number[] = [];
let animFrameId: number | null = null;

// ═══ Gaussian magnification (like macOS) ═══
function gaussian(dist: number): number {
  const sigma = MAGNIFY_RANGE / 2.5;
  return Math.exp(-(dist * dist) / (2 * sigma * sigma));
}

function updateTargetScales() {
  if (!dockBarEl) return;

  const barRect = dockBarEl.getBoundingClientRect();

  for (let i = 0; i < dockItems.length; i++) {
    const item = dockItems[i];
    const itemRect = item.getBoundingClientRect();
    const itemCenterX = itemRect.left + itemRect.width / 2;

    if (isHovering && mouseX > barRect.left - 40 && mouseX < barRect.right + 40) {
      const dist = Math.abs(mouseX - itemCenterX);
      targetScales[i] = 1 + (MAX_SCALE - 1) * gaussian(dist);
    } else {
      targetScales[i] = 1;
    }
  }
}

function applyScales() {
  let needsFrame = false;

  for (let i = 0; i < dockItems.length; i++) {
    const diff = targetScales[i] - currentScales[i];
    const speed = isHovering ? LERP_SPEED : SPRING_SPEED;

    if (Math.abs(diff) > 0.001) {
      currentScales[i] += diff * speed;
      needsFrame = true;
    } else {
      currentScales[i] = targetScales[i];
    }

    const s = currentScales[i];
    const item = dockItems[i];
    const newSize = BASE_SIZE * s;

    item.style.width = `${newSize}px`;
    item.style.height = `${newSize}px`;

    // Add/remove magnified class for enhanced shadow
    if (s > 1.15) {
      item.classList.add("magnified");
    } else {
      item.classList.remove("magnified");
    }
  }

  if (needsFrame) {
    animFrameId = requestAnimationFrame(applyScales);
  } else {
    animFrameId = null;
  }
}

function startAnimation() {
  if (animFrameId === null) {
    animFrameId = requestAnimationFrame(applyScales);
  }
}

// ═══ Bootstrap ═══
async function bootstrap() {
  dockBarEl = document.getElementById("dock-bar");
  if (!dockBarEl) return;

  try {
    const config: Config = await invoke("get_config");
    let isFirstCategory = true;

    for (const category of config.categories) {
      // Add separator between categories
      if (!isFirstCategory) {
        const sep = document.createElement("div");
        sep.className = "dock-separator";
        dockBarEl.appendChild(sep);
      }
      isFirstCategory = false;

      for (const shortcut of category.shortcuts) {
        const itemEl = document.createElement("div");
        itemEl.className = "dock-item";
        itemEl.setAttribute("data-name", shortcut.name);
        itemEl.style.width = `${BASE_SIZE}px`;
        itemEl.style.height = `${BASE_SIZE}px`;

        // Click to launch with bounce animation
        const appPath = shortcut.path;
        itemEl.addEventListener("click", () => {
          itemEl.classList.add("bouncing");
          itemEl.addEventListener("animationend", () => {
            itemEl.classList.remove("bouncing");
          }, { once: true });
          invoke("launch_app", { path: appPath }).catch((err: unknown) =>
            console.error("Launch failed:", err)
          );
        });

        const imgEl = document.createElement("img");
        imgEl.alt = shortcut.name;
        imgEl.draggable = false;
        itemEl.appendChild(imgEl);
        dockBarEl.appendChild(itemEl);
        dockItems.push(itemEl);

        // Initialize scales
        currentScales.push(1);
        targetScales.push(1);

        // Async icon loading
        invoke("get_icon_base64", { path: shortcut.path })
          .then((base64: unknown) => {
            if (typeof base64 === "string") {
              imgEl.src = base64;
            } else {
              // SVG placeholder for missing icons
              imgEl.src = createPlaceholderSVG(shortcut.name);
            }
          })
          .catch(() => {
            imgEl.src = createPlaceholderSVG(shortcut.name);
          });
      }
    }

    // ─── Mouse tracking ───
    dockBarEl.addEventListener("mousemove", (e: MouseEvent) => {
      mouseX = e.clientX;
      isHovering = true;
      updateTargetScales();
      startAnimation();
    });

    dockBarEl.addEventListener("mouseleave", () => {
      isHovering = false;
      updateTargetScales();
      startAnimation();
    });

    // Track mouse even outside dock for smooth exit
    document.addEventListener("mousemove", (e: MouseEvent) => {
      if (!isHovering) return;
      mouseX = e.clientX;
      updateTargetScales();
    });

  } catch (err) {
    console.error("Failed to load dock configuration", err);
  }
}

// ─── Placeholder icon for missing executables ───
function createPlaceholderSVG(name: string): string {
  const letter = name.charAt(0).toUpperCase();
  const colors = ["#4285F4", "#EA4335", "#FBBC04", "#34A853", "#FF6D01", "#46BDC6"];
  const color = colors[name.length % colors.length];

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
    <defs>
      <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stop-color="${color}"/>
        <stop offset="100%" stop-color="${color}88"/>
      </linearGradient>
    </defs>
    <rect width="64" height="64" rx="14" fill="url(#g)"/>
    <text x="32" y="42" text-anchor="middle" fill="white" font-size="28" font-weight="600" font-family="Inter, sans-serif">${letter}</text>
  </svg>`;

  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

window.addEventListener("DOMContentLoaded", () => {
  bootstrap();
});
