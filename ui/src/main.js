const { invoke } = window.__TAURI__.core;

async function loadSettings() {
  // Defaults used until a saved config overrides them: auto-save on, no clipboard copy.
  document.querySelector("#auto-save").checked = true;
  document.querySelector("#copy-to-clipboard").checked = false;

  try {
    const config = await invoke("load_config");
    if (config) {
      document.querySelector("#save-directory").value = config.save_directory ?? "";
      document.querySelector("#image-format").value = config.image_format ?? "png";
      document.querySelector("#auto-save").checked = config.auto_save ?? true;
      document.querySelector("#copy-to-clipboard").checked = config.copy_to_clipboard ?? false;
    }
  } catch (err) {
    // Leave the defaults in place if the config cannot be read.
  }
}

async function saveSettings() {
  const config = {
    save_directory: document.querySelector("#save-directory").value.trim(),
    image_format: document.querySelector("#image-format").value,
    auto_save: document.querySelector("#auto-save").checked,
    copy_to_clipboard: document.querySelector("#copy-to-clipboard").checked,
  };

  const statusEl = document.querySelector("#settings-status");
  try {
    const saved = await invoke("save_config", { config });
    statusEl.textContent = saved
      ? "Saved to config/app.cfg"
      : "Failed to save settings.";
  } catch (err) {
    statusEl.textContent = `Error: ${err}`;
  }
}

async function browseDirectory() {
  const statusEl = document.querySelector("#settings-status");
  const open = window.__TAURI__?.dialog?.open;
  if (!open) {
    statusEl.textContent = "Folder picker is unavailable; type a path instead.";
    return;
  }

  try {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      document.querySelector("#save-directory").value = selected;
    }
  } catch (err) {
    statusEl.textContent = `Error: ${err}`;
  }
}

function restoreDefaultDirectory() {
  document.querySelector("#save-directory").value = "";
  saveSettings();
}

async function captureNow() {
  try {
    await invoke("start_free_roam_capture");
  } catch (err) {
    document.querySelector(".status").textContent = `Error: ${err}`;
  }
}

function showTab(name) {
  const isCapture = name === "capture";
  const tabs = {
    capture: { tab: "#tab-capture", panel: "#capture-tab", active: isCapture },
    settings: { tab: "#tab-settings", panel: "#settings-tab", active: !isCapture },
  };

  for (const { tab, panel, active } of Object.values(tabs)) {
    const tabEl = document.querySelector(tab);
    const panelEl = document.querySelector(panel);
    tabEl.classList.toggle("is-active", active);
    tabEl.setAttribute("aria-selected", String(active));
    panelEl.classList.toggle("is-active", active);
    panelEl.hidden = !active;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  loadSettings();

  document.querySelector("#settings-form").addEventListener("submit", (event) => {
    event.preventDefault();
    saveSettings();
  });

  document.querySelector("#browse-directory").addEventListener("click", browseDirectory);
  document.querySelector("#restore-directory").addEventListener("click", restoreDefaultDirectory);
  document.querySelector("#capture-now").addEventListener("click", captureNow);

  document.querySelector("#tab-capture").addEventListener("click", () => showTab("capture"));
  document.querySelector("#tab-settings").addEventListener("click", () => showTab("settings"));
});
