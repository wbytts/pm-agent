(function () {
  "use strict";

  const raw = document.getElementById("session-data").textContent || "{}";
  const data = JSON.parse(raw);
  const header = data.header || {};
  const entries = Array.isArray(data.entries) ? data.entries : [];
  const messages = document.getElementById("messages");

  document.getElementById("session-title").textContent = header.name || "PM Agent Session";
  document.getElementById("session-meta").textContent = [
    header.id ? `id: ${header.id}` : null,
    header.cwd ? `cwd: ${header.cwd}` : null,
    data.leafId ? `leaf: ${data.leafId}` : null,
  ].filter(Boolean).join(" · ");
  document.getElementById("theme-name").textContent = data.theme || "dark";

  if (entries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "No session entries";
    messages.appendChild(empty);
    return;
  }

  for (const entry of entries) {
    messages.appendChild(renderEntry(entry));
  }

  function renderEntry(entry) {
    const item = document.createElement("article");
    const role = entry.message && entry.message.role ? String(entry.message.role).toLowerCase() : "info";
    item.className = `entry ${role}`;

    const header = document.createElement("div");
    header.className = "entry-header";
    const title = document.createElement("span");
    title.textContent = entryTitle(entry);
    const time = document.createElement("span");
    time.textContent = entry.timestamp || "";
    header.append(title, time);

    const body = document.createElement("div");
    body.className = "entry-body";
    body.textContent = entryText(entry);

    item.append(header, body);
    return item;
  }

  function entryTitle(entry) {
    if (entry.type === "message" && entry.message) {
      return `${entry.message.role || "message"} · ${entry.id}`;
    }
    return `${entry.type || "entry"} · ${entry.id || ""}`;
  }

  function entryText(entry) {
    if (entry.type === "message" && entry.message) {
      return entry.message.content || "";
    }
    if (entry.type === "session_info") {
      return entry.name || "";
    }
    if (entry.type === "compaction" || entry.type === "branch_summary") {
      return entry.summary || "";
    }
    if (entry.type === "model_change") {
      return `${entry.provider || ""}/${entry.modelId || ""}`;
    }
    if (entry.type === "thinking_level_change") {
      return entry.thinkingLevel || "";
    }
    if (entry.type === "custom_message") {
      return entry.content || "";
    }
    return JSON.stringify(entry, null, 2);
  }
})();
