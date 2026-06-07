const { invoke } = window.__TAURI__.core;

const customDataSelectors = {
  exif: "#custom-exif",
  metadata: "#custom-metadata",
};

async function loadSettings() {
  // Defaults used until a saved config overrides them: auto-save on, no clipboard copy.
  document.querySelector("#auto-save").checked = true;
  document.querySelector("#copy-to-clipboard").checked = false;
  document.querySelector("#circular-selection").checked = false;
  document.querySelector("#ignore-self").checked = false;
  document.querySelector("#fill-custom-data").checked = false;
  setCustomData({ exif: "", metadata: "" });

  try {
    const config = await invoke("load_config");
    if (config) {
      document.querySelector("#save-directory").value = config.save_directory ?? "";
      document.querySelector("#image-format").value = config.image_format ?? "png";
      document.querySelector("#auto-save").checked = config.auto_save ?? true;
      document.querySelector("#copy-to-clipboard").checked = config.copy_to_clipboard ?? false;
      document.querySelector("#circular-selection").checked = config.circular_selection ?? false;
      document.querySelector("#ignore-self").checked = config.ignore_self ?? false;
      document.querySelector("#fill-custom-data").checked = config.fill_custom_data ?? false;
      setCustomData(config.custom_data);
    }
  } catch (err) {
    // Leave the defaults in place if the config cannot be read.
  }
}

async function saveSettings(statusSelector = "#settings-status", successText = "Saved to config/app.cfg") {
  const config = {
    save_directory: document.querySelector("#save-directory").value.trim(),
    image_format: document.querySelector("#image-format").value,
    auto_save: document.querySelector("#auto-save").checked,
    copy_to_clipboard: document.querySelector("#copy-to-clipboard").checked,
    circular_selection: document.querySelector("#circular-selection").checked,
    ignore_self: document.querySelector("#ignore-self").checked,
    fill_custom_data: document.querySelector("#fill-custom-data").checked,
    custom_data: getCustomData(),
  };

  const statusEl = document.querySelector(statusSelector);
  try {
    const saved = await invoke("save_config", { config });
    statusEl.textContent = saved
      ? successText
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

function getCustomData() {
  return Object.fromEntries(
    Object.entries(customDataSelectors).map(([key, selector]) => [
      key,
      document.querySelector(selector).value.trim(),
    ])
  );
}

function setCustomData(customData = {}) {
  for (const [key, selector] of Object.entries(customDataSelectors)) {
    document.querySelector(selector).value = customData?.[key] ?? "";
  }
}

async function generateCustomData() {
  const statusEl = document.querySelector("#custom-data-status");

  try {
    const customData = await invoke("generate_custom_data");
    setCustomData(customData);
    await saveSettings("#custom-data-status", "Generated and saved custom data.");
  } catch (err) {
    statusEl.textContent = `Error: ${err}`;
  }
}

async function captureNow() {
  try {
    await invoke("start_free_roam_capture");
  } catch (err) {
    document.querySelector(".status").textContent = `Error: ${err}`;
  }
}

function showTab(name) {
  const tabs = {
    capture: { tab: "#tab-capture", panel: "#capture-tab", active: name === "capture" },
    settings: { tab: "#tab-settings", panel: "#settings-tab", active: name === "settings" },
    customData: { tab: "#tab-custom-data", panel: "#custom-data-tab", active: name === "custom-data" },
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
    saveSettings("#settings-status");
  });

  document.querySelector("#custom-data-form").addEventListener("submit", (event) => {
    event.preventDefault();
    saveSettings("#custom-data-status", "Saved custom data to config/app.cfg");
  });

  document.querySelector("#browse-directory").addEventListener("click", browseDirectory);
  document.querySelector("#restore-directory").addEventListener("click", restoreDefaultDirectory);
  document.querySelector("#generate-custom-data").addEventListener("click", generateCustomData);
  document.querySelector("#capture-now").addEventListener("click", captureNow);

  document.querySelector("#tab-capture").addEventListener("click", () => showTab("capture"));
  document.querySelector("#tab-settings").addEventListener("click", () => showTab("settings"));
  document.querySelector("#tab-custom-data").addEventListener("click", () => showTab("custom-data"));
});
