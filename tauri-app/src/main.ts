import { invoke } from "@tauri-apps/api/core";

// Interfaces holding config data from Rust
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

let dockBarEl: HTMLElement | null = null;

async function bootstrap() {
  dockBarEl = document.getElementById("dock-bar");
  if (!dockBarEl) return;

  try {
    // 1. Fetch Config from Rust
    const config: Config = await invoke("get_config");

    // 2. Loop through every shortcut in every category
    for (const category of config.categories) {
      for (const shortcut of category.shortcuts) {

        // Setup Icon HTML wrapper
        const itemEl = document.createElement("div");
        itemEl.className = "dock-item";
        itemEl.title = shortcut.name; // Basic tooltip

        // Create img tag placeholder
        const imgEl = document.createElement("img");
        itemEl.appendChild(imgEl);
        dockBarEl.appendChild(itemEl);

        // 3. Fetch Base64 icon async for each item
        invoke("get_icon_base64", { path: shortcut.path })
          .then((base64: unknown) => {
            if (typeof base64 === 'string') {
              imgEl.src = base64;
            } else {
              console.warn(`Failed to extract icon for: ${shortcut.path}`);
              // Fallback placeholder logic
              imgEl.src = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0OCIgaGVpZ2h0PSI0OCIgdmlld0JveD0iMCAwIDI0IDI0Ij48cGF0aCBmaWxsPSIjY2NjIiBkPSJNMTQgMkg2YTIgMiAwIDAgMC0yIDJ2MTZhMiAyIDAgMCAwIDIgMmgxMmEy MiAwIDAgMCAyLTJWOTh6bTQgMThINlY0aDd2NWg1em0tMy04SDl2Mmg2em0tMyA0SDl2MmgzeiIvPjwvc3ZnPg==";
            }
          })
          .catch((err) => {
            console.error(err);
          });
      }
    }
  } catch (err) {
    console.error("Failed to load dock configuration", err);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  bootstrap();
});
