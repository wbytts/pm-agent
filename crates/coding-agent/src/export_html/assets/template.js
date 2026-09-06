(function () {
  "use strict";

  const base64 = document.getElementById("session-data").textContent || "";
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  const data = JSON.parse(new TextDecoder("utf-8").decode(bytes));
  const header = data.header || {};
  const entries = Array.isArray(data.entries) ? data.entries : [];
  const systemPrompt = data.systemPrompt;
  const tools = Array.isArray(data.tools) ? data.tools : [];
  const renderedTools = data.renderedTools && typeof data.renderedTools === "object" ? data.renderedTools : {};
  const messages = document.getElementById("messages");
  const urlParams = new URLSearchParams(window.location.search.substring(1));
  const urlLeafId = urlParams.get("leafId");
  const targetId = urlParams.get("targetId");
  const byId = new Map(entries.filter((entry) => entry.id).map((entry) => [entry.id, entry]));
  const labelMap = new Map();
  for (const entry of entries) {
    if (entry.type === "label" && entry.targetId && entry.label) {
      labelMap.set(entry.targetId, entry.label);
    }
  }
  let currentLeafId = urlLeafId || data.leafId || (entries.length > 0 ? entries[entries.length - 1].id : null);
  let currentTargetId = targetId || currentLeafId;
  let thinkingExpanded = true;
  let toolOutputsExpanded = false;
  let filterMode = "all";
  let searchQuery = "";

  document.getElementById("session-title").textContent = header.name || "PM Agent Session";
  document.getElementById("session-meta").textContent = [
    header.id ? `id: ${header.id}` : null,
    header.cwd ? `cwd: ${header.cwd}` : null,
    data.leafId ? `leaf: ${data.leafId}` : null,
  ].filter(Boolean).join(" · ");
  document.getElementById("theme-name").textContent = data.theme || "dark";
  renderSessionInfo();

  if (entries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "No session entries";
    messages.appendChild(empty);
    return;
  }

  attachCopyLinkHandlers();
  attachHeaderHandlers();
  attachFilterHandlers();
  attachImageModalHandlers();
  renderSidebarTree();
  navigateTo(currentLeafId, currentTargetId ? "target" : "none", currentTargetId);

  function renderEntry(entry) {
    const item = document.createElement("article");
    const role = entry.message && entry.message.role ? String(entry.message.role).toLowerCase() : "info";
    item.className = `entry ${role}`;
    if (entry.id) {
      item.id = entry.id;
      item.dataset.entryId = entry.id;
    }

    const header = document.createElement("div");
    header.className = "entry-header";
    const title = document.createElement("span");
    title.textContent = entryTitle(entry);
    const time = document.createElement("span");
    time.textContent = entry.timestamp || "";
    const copyLink = document.createElement("button");
    copyLink.type = "button";
    copyLink.className = "copy-link-btn";
    copyLink.dataset.entryId = entry.id || "";
    copyLink.title = "Copy link to this entry";
    copyLink.setAttribute("aria-label", "Copy link to this entry");
    copyLink.textContent = "Link";
    header.append(title, time, copyLink);

    const body = document.createElement("div");
    body.className = "entry-body";
    renderEntryBody(body, entry);

    item.append(header, body);
    return item;
  }

  function buildEntryTree() {
    const nodes = new Map();
    const roots = [];
    for (const entry of entries) {
      if (!entry.id) {
        continue;
      }
      nodes.set(entry.id, { entry, children: [], label: labelMap.get(entry.id) });
    }
    for (const node of nodes.values()) {
      const parentId = node.entry.parentId;
      const parent = parentId && parentId !== node.entry.id ? nodes.get(parentId) : null;
      if (parent) {
        parent.children.push(node);
      } else {
        roots.push(node);
      }
    }
    const sortNodes = (items) => {
      items.sort((left, right) => String(left.entry.timestamp || "").localeCompare(String(right.entry.timestamp || "")));
      for (const item of items) {
        sortNodes(item.children);
      }
    };
    sortNodes(roots);
    return roots;
  }

  function renderSidebarTree() {
    const tree = document.getElementById("sidebar-tree");
    if (!tree) {
      return;
    }
    tree.innerHTML = "";
    for (const node of buildEntryTree()) {
      renderSidebarTreeNode(tree, node, 0);
    }
  }

  function renderSidebarTreeNode(parent, node, depth) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "sidebar-tree-node";
    button.dataset.entryId = node.entry.id || "";
    button.style.setProperty("--depth", String(depth));
    button.innerHTML = sidebarEntryLabel(node.entry);
    button.addEventListener("click", () => navigateTo(findNewestLeaf(node.entry.id), "target", node.entry.id));
    parent.appendChild(button);
    for (const child of node.children) {
      renderSidebarTreeNode(parent, child, depth + 1);
    }
  }

  function buildActivePathIds(targetId) {
    return new Set(getPath(targetId).map((entry) => entry.id));
  }

  function getPath(targetId) {
    const path = [];
    let current = targetId ? byId.get(targetId) : null;
    while (current) {
      path.unshift(current);
      if (!current.parentId || current.parentId === current.id) {
        break;
      }
      current = byId.get(current.parentId);
    }
    return path;
  }

  function findNewestLeaf(entryId) {
    let current = byId.get(entryId);
    if (!current) {
      return entryId;
    }
    while (true) {
      const children = entries
        .filter((entry) => entry.parentId === current.id && entry.id !== current.id)
        .sort((left, right) => String(left.timestamp || "").localeCompare(String(right.timestamp || "")));
      if (children.length === 0) {
        return current.id;
      }
      current = children[children.length - 1];
    }
  }

  function navigateTo(leafId, scrollMode = "target", scrollToEntryId = null) {
    const leaf = leafId && byId.has(leafId) ? leafId : (entries.length > 0 ? entries[entries.length - 1].id : null);
    currentLeafId = leaf;
    currentTargetId = scrollToEntryId || leaf;
    messages.innerHTML = "";
    for (const entry of getPath(currentLeafId)) {
      messages.appendChild(renderEntry(entry));
    }
    attachCopyLinkHandlers();
    attachImageModalHandlers();
    applyEntryFilters();
    markActiveSidebarPath();
    if (scrollMode === "target" && currentTargetId) {
      scrollToEntry(currentTargetId);
    }
  }

  function markActiveSidebarPath() {
    const activePathIds = buildActivePathIds(currentLeafId);
    document.querySelectorAll(".sidebar-tree-node").forEach((node) => {
      const entryId = node.dataset.entryId;
      node.classList.toggle("sidebar-active-path", activePathIds.has(entryId));
      node.classList.toggle("sidebar-current-leaf", entryId === currentLeafId);
    });
  }

  function sidebarEntryLabel(entry) {
    const label = entryLabel(entry);
    const labelHtml = label ? `<span class="sidebar-label">[${escapeHtml(label)}]</span> ` : "";
    if (entry.type === "message" && entry.message) {
      const role = entry.message.role || "message";
      const text = messageText(entry.message.content).trim().replace(/\s+/g, " ");
      const toolCall = firstToolCall(entry.message.content);
      if (!text && toolCall) {
        return labelHtml + escapeHtml(formatToolCall(toolCall.name, toolCall.arguments));
      }
      return labelHtml + escapeHtml(text ? `${role}: ${text.slice(0, 60)}` : role);
    }
    if (entry.type === "label") {
      return `label: ${escapeHtml(entry.label || entry.targetId || "")}`;
    }
    return labelHtml + escapeHtml(`${entry.type || "entry"}: ${entry.id || ""}`);
  }

  function entryLabel(entry) {
    return entry && entry.id ? labelMap.get(entry.id) : undefined;
  }

  function scrollToEntry(entryId) {
    const entry = entryId ? document.getElementById(entryId) : null;
    if (!entry) {
      return;
    }
    entry.classList.add("entry-target");
    entry.scrollIntoView({ block: "center" });
    setTimeout(() => entry.classList.remove("entry-target"), 2000);
  }

  function computeStats(entryList) {
    const stats = {
      userMessages: 0,
      assistantMessages: 0,
      toolResults: 0,
      customMessages: 0,
      compactions: 0,
      branchSummaries: 0,
      toolCalls: 0,
      tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      models: new Set(),
    };

    for (const entry of entryList) {
      if (entry.type === "message" && entry.message) {
        const msg = entry.message;
        const role = String(msg.role || "").toLowerCase();
        if (role === "user") {
          stats.userMessages++;
        } else if (role === "assistant") {
          stats.assistantMessages++;
          if (msg.model) {
            stats.models.add(msg.provider ? `${msg.provider}/${msg.model}` : msg.model);
          }
          addUsage(stats, msg.usage);
          if (Array.isArray(msg.content)) {
            stats.toolCalls += msg.content.filter((block) => block && block.type === "toolCall").length;
          }
        } else if (role === "toolresult") {
          stats.toolResults++;
        }
      } else if (entry.type === "compaction") {
        stats.compactions++;
      } else if (entry.type === "branch_summary") {
        stats.branchSummaries++;
      } else if (entry.type === "custom_message") {
        stats.customMessages++;
      }
    }

    return stats;
  }

  function addUsage(stats, usage) {
    if (!usage || typeof usage !== "object") {
      return;
    }
    stats.tokens.input += Number(usage.input || 0);
    stats.tokens.output += Number(usage.output || 0);
    stats.tokens.cacheRead += Number(usage.cacheRead || 0);
    stats.tokens.cacheWrite += Number(usage.cacheWrite || 0);

    if (usage.cost && typeof usage.cost === "object") {
      stats.cost.input += Number(usage.cost.input || 0);
      stats.cost.output += Number(usage.cost.output || 0);
      stats.cost.cacheRead += Number(usage.cost.cacheRead || 0);
      stats.cost.cacheWrite += Number(usage.cost.cacheWrite || 0);
    }
  }

  function renderSessionInfo() {
    const stats = computeStats(entries);
    const container = document.createElement("section");
    container.className = "session-info";

    const msgParts = [
      stats.userMessages ? `${stats.userMessages} user` : null,
      stats.assistantMessages ? `${stats.assistantMessages} assistant` : null,
      stats.toolResults ? `${stats.toolResults} tool results` : null,
      stats.customMessages ? `${stats.customMessages} custom` : null,
      stats.compactions ? `${stats.compactions} compactions` : null,
      stats.branchSummaries ? `${stats.branchSummaries} branch summaries` : null,
    ].filter(Boolean);
    const tokenParts = [
      stats.tokens.input ? `in ${formatCompactNumber(stats.tokens.input)}` : null,
      stats.tokens.output ? `out ${formatCompactNumber(stats.tokens.output)}` : null,
      stats.tokens.cacheRead ? `cache read ${formatCompactNumber(stats.tokens.cacheRead)}` : null,
      stats.tokens.cacheWrite ? `cache write ${formatCompactNumber(stats.tokens.cacheWrite)}` : null,
    ].filter(Boolean);
    const totalCost = stats.cost.input + stats.cost.output + stats.cost.cacheRead + stats.cost.cacheWrite;

    appendInfoItem(container, "Date", header.timestamp || header.createdAt || "unknown");
    appendInfoItem(container, "Models", Array.from(stats.models).join(", ") || "unknown");
    appendInfoItem(container, "Messages", msgParts.join(", ") || "0");
    appendInfoItem(container, "Tool Calls", String(stats.toolCalls));
    appendInfoItem(container, "Tokens", tokenParts.join(" ") || "0");
    appendInfoItem(container, "Cost", `$${totalCost.toFixed(3)}`);

    const appHeader = document.querySelector(".app-header");
    appHeader.insertAdjacentElement("afterend", container);

    if (systemPrompt) {
      appHeader.insertAdjacentElement("afterend", renderSystemPrompt(systemPrompt));
    }
    if (tools.length > 0) {
      container.insertAdjacentElement("afterend", renderToolsList(tools));
    }
  }

  function appendInfoItem(parent, label, value) {
    const item = document.createElement("div");
    item.className = "session-info-item";
    const name = document.createElement("span");
    name.className = "session-info-label";
    name.textContent = `${label}:`;
    const text = document.createElement("span");
    text.className = "session-info-value";
    text.textContent = value;
    item.append(name, text);
    parent.appendChild(item);
  }

  function renderSystemPrompt(prompt) {
    const node = document.createElement("section");
    node.className = "system-prompt";
    node.addEventListener("click", () => {
      if (window.getSelection().toString()) {
        return;
      }
      node.classList.toggle("expanded");
    });

    const title = document.createElement("div");
    title.className = "system-prompt-header";
    title.textContent = "System Prompt";
    const body = document.createElement("pre");
    body.className = "system-prompt-body";
    body.textContent = prompt;
    node.append(title, body);
    return node;
  }

  function renderToolsList(toolDefinitions) {
    const node = document.createElement("section");
    node.className = "tools-list";
    const title = document.createElement("div");
    title.className = "tools-header";
    title.textContent = "Available Tools";
    const content = document.createElement("div");
    content.className = "tools-content";

    for (const tool of toolDefinitions) {
      content.appendChild(renderToolDefinition(tool));
    }

    node.append(title, content);
    return node;
  }

  function renderToolDefinition(tool) {
    const item = document.createElement("div");
    item.className = "tool-item";
    item.addEventListener("click", () => {
      if (window.getSelection().toString()) {
        return;
      }
      item.classList.toggle("params-expanded");
    });

    const name = document.createElement("span");
    name.className = "tool-item-name";
    name.textContent = tool.name || "tool";
    const description = document.createElement("span");
    description.className = "tool-item-desc";
    description.textContent = tool.description ? ` - ${tool.description}` : "";
    item.append(name, description);

    const params = tool.parameters;
    const properties = params && typeof params === "object" ? params.properties : null;
    if (!properties || Object.keys(properties).length === 0) {
      return item;
    }

    const paramsContent = document.createElement("div");
    paramsContent.className = "tool-params-content";
    const required = Array.isArray(params.required) ? params.required : [];
    for (const [paramName, property] of Object.entries(properties)) {
      paramsContent.appendChild(renderToolParam(paramName, property || {}, required.includes(paramName)));
    }
    item.appendChild(paramsContent);
    return item;
  }

  function renderToolParam(name, property, isRequired) {
    const param = document.createElement("div");
    param.className = "tool-param";
    const paramName = document.createElement("span");
    paramName.className = "tool-param-name";
    paramName.textContent = name;
    const type = document.createElement("span");
    type.className = "tool-param-type";
    type.textContent = property.type || "any";
    const required = document.createElement("span");
    required.className = isRequired ? "tool-param-required" : "tool-param-optional";
    required.textContent = isRequired ? "required" : "optional";
    param.append(paramName, type, required);

    if (property.description) {
      const description = document.createElement("div");
      description.className = "tool-param-desc";
      description.textContent = property.description;
      param.appendChild(description);
    }
    return param;
  }

  function formatCompactNumber(value) {
    if (value >= 1000000) {
      return `${(value / 1000000).toFixed(1)}M`;
    }
    if (value >= 1000) {
      return `${(value / 1000).toFixed(1)}K`;
    }
    return String(value);
  }

  function attachCopyLinkHandlers() {
    messages.querySelectorAll(".copy-link-btn").forEach((button) => {
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        copyEntryLink(button.dataset.entryId, button);
      });
    });
  }

  function attachHeaderHandlers() {
    document.querySelector("[data-action=\"toggle-thinking\"]")?.addEventListener("click", toggleThinking);
    document.querySelector("[data-action=\"toggle-tools\"]")?.addEventListener("click", toggleToolOutputs);
    document.addEventListener("keydown", (event) => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      const target = event.target;
      const tagName = target && target.tagName ? target.tagName.toLowerCase() : "";
      if (tagName === "input" || tagName === "textarea" || tagName === "select" || target?.isContentEditable) {
        return;
      }
      if (event.key === "t" || event.key === "T") {
        toggleThinking();
      } else if (event.key === "o" || event.key === "O") {
        toggleToolOutputs();
      } else if (event.key === "Escape") {
        closeImageModal();
        const search = document.getElementById("entry-search");
        if (search && search.value) {
          search.value = "";
          searchQuery = "";
          applyEntryFilters();
        }
      }
    });
  }

  function attachImageModalHandlers() {
    messages.querySelectorAll(".message-image, .tool-image").forEach((image) => {
      if (image.dataset.modalBound === "true") {
        return;
      }
      image.dataset.modalBound = "true";
      image.addEventListener("click", () => openImageModal(image.src));
    });

    const modal = document.getElementById("image-modal");
    if (modal && modal.dataset.modalBound !== "true") {
      modal.dataset.modalBound = "true";
      modal.addEventListener("click", closeImageModal);
    }
  }

  function openImageModal(src) {
    const modal = document.getElementById("image-modal");
    const image = document.getElementById("modal-image");
    if (!modal || !image || !src) {
      return;
    }
    image.src = src;
    modal.classList.add("open");
    modal.setAttribute("aria-hidden", "false");
  }

  function closeImageModal() {
    const modal = document.getElementById("image-modal");
    const image = document.getElementById("modal-image");
    if (!modal || !image) {
      return;
    }
    modal.classList.remove("open");
    modal.setAttribute("aria-hidden", "true");
    image.removeAttribute("src");
  }

  function attachFilterHandlers() {
    const search = document.getElementById("entry-search");
    search?.addEventListener("input", (event) => {
      searchQuery = event.target.value || "";
      applyEntryFilters();
    });
    document.querySelectorAll(".filter-btn").forEach((button) => {
      button.addEventListener("click", () => {
        document.querySelectorAll(".filter-btn").forEach((candidate) => candidate.classList.remove("active"));
        button.classList.add("active");
        filterMode = button.dataset.filter || "all";
        applyEntryFilters();
      });
    });
  }

  function applyEntryFilters() {
    const terms = searchQuery.toLowerCase().split(/\s+/).filter(Boolean);
    let visible = 0;
    const entryNodes = messages.querySelectorAll(".entry");
    entryNodes.forEach((node) => {
      const entry = entries.find((candidate) => candidate.id === node.dataset.entryId);
      const matched = Boolean(entry && entryMatchesFilter(entry, terms));
      node.classList.toggle("entry-filter-hidden", !matched);
      if (matched) {
        visible++;
      }
    });
    const status = document.getElementById("entry-filter-status");
    if (status) {
      status.textContent = `${visible} / ${entryNodes.length} entries`;
    }
    syncSidebarFilters(terms);
  }

  function syncSidebarFilters(terms) {
    const activePathIds = buildActivePathIds(currentLeafId);
    let sidebarVisible = 0;
    document.querySelectorAll(".sidebar-tree-node").forEach((node) => {
      const entry = byId.get(node.dataset.entryId);
      const matched = Boolean(entry && entryMatchesFilter(entry, terms));
      const keepActivePath = activePathIds.has(node.dataset.entryId);
      const visible = matched || keepActivePath;
      node.classList.toggle("sidebar-filter-hidden", !visible);
      if (visible) {
        sidebarVisible++;
      }
    });
    const status = document.getElementById("entry-filter-status");
    if (status) {
      const entryVisible = messages.querySelectorAll(".entry:not(.entry-filter-hidden)").length;
      status.textContent = `${entryVisible} / ${messages.querySelectorAll(".entry").length} entries · ${sidebarVisible} / ${entries.length} tree`;
    }
  }

  function entryMatchesFilter(entry, terms) {
    const role = messageRole(entry);
    if (filterMode === "user-only" && !(entry.type === "message" && role === "user")) {
      return false;
    }
    if (filterMode === "no-tools" && isToolEntry(entry)) {
      return false;
    }
    if (filterMode === "labeled-only" && !entryLabel(entry)) {
      return false;
    }
    if (terms.length === 0) {
      return true;
    }
    const text = entrySearchText(entry).toLowerCase();
    return terms.every((term) => text.includes(term));
  }

  function isToolEntry(entry) {
    const role = messageRole(entry);
    if (entry.type === "message" && role === "toolresult") {
      return true;
    }
    if (entry.type === "message" && entry.message && Array.isArray(entry.message.content)) {
      return entry.message.content.some((block) => block && block.type === "toolCall");
    }
    return false;
  }

  function entrySearchText(entry) {
    const parts = [entry.type, entry.id, entry.timestamp, entryLabel(entry), entryText(entry)];
    if (entry.type === "message" && entry.message) {
      parts.push(entry.message.role, entry.message.provider, entry.message.model, messageText(entry.message.content));
    }
    return parts.filter(Boolean).join(" ");
  }

  function toggleThinking() {
    thinkingExpanded = !thinkingExpanded;
    document.querySelectorAll(".thinking-text").forEach((node) => {
      node.style.display = thinkingExpanded ? "" : "none";
    });
    document.querySelectorAll(".thinking-collapsed").forEach((node) => {
      node.style.display = thinkingExpanded ? "none" : "block";
    });
  }

  function toggleToolOutputs() {
    toolOutputsExpanded = !toolOutputsExpanded;
    document.querySelectorAll(".tool-output.expandable").forEach((node) => {
      node.classList.toggle("expanded", toolOutputsExpanded);
    });
  }

  async function copyEntryLink(entryId, button) {
    if (!entryId) {
      return;
    }
    const url = new URL(window.location.href);
    url.searchParams.set("targetId", entryId);
    if (currentLeafId) {
      url.searchParams.set("leafId", currentLeafId);
    }

    let copied = false;
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        await navigator.clipboard.writeText(url.toString());
        copied = true;
      }
    } catch {
      copied = false;
    }

    if (!copied) {
      copied = copyTextWithTextarea(url.toString());
    }

    if (copied && button) {
      const originalText = button.textContent;
      button.textContent = "Copied";
      button.classList.add("copied");
      setTimeout(() => {
        button.textContent = originalText;
        button.classList.remove("copied");
      }, 1500);
    }
  }

  function copyTextWithTextarea(text) {
    try {
      const textarea = document.createElement("textarea");
      textarea.value = text;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      const copied = document.execCommand("copy");
      document.body.removeChild(textarea);
      return copied;
    } catch {
      return false;
    }
  }

  function scrollToTargetEntry() {
    if (!targetId) {
      return;
    }
    scrollToEntry(targetId);
  }

  function entryTitle(entry) {
    if (entry.type === "message" && entry.message) {
      return `${entry.message.role || "message"} · ${entry.id}`;
    }
    return `${entry.type || "entry"} · ${entry.id || ""}`;
  }

  function escapeHtml(value) {
    return String(value)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function isSafeUrl(value) {
    const href = String(value || "").trim().toLowerCase();
    return !href.startsWith("javascript:") && !href.startsWith("vbscript:") && !href.startsWith("data:");
  }

  const markdownRenderer = {
    link(token) {
      const href = token && token.href ? String(token.href) : "";
      const text = token && token.text ? String(token.text) : href;
      if (!isSafeUrl(href)) {
        return escapeHtml(text);
      }
      const title = token && token.title ? ` title="${escapeHtml(token.title)}"` : "";
      const tokens = token && token.tokens ? this.parser.parseInline(token.tokens) : escapeHtml(text);
      return `<a href="${escapeHtml(href)}"${title} rel="noreferrer noopener" target="_blank">${tokens}</a>`;
    },
    image(token) {
      const href = token && token.href ? String(token.href) : "";
      if (!isSafeUrl(href)) {
        return escapeHtml(token && token.text ? token.text : "");
      }
      const text = token && token.text ? String(token.text) : "";
      const title = token && token.title ? ` title="${escapeHtml(token.title)}"` : "";
      return `<img src="${escapeHtml(href)}" alt="${escapeHtml(text)}"${title}>`;
    },
    code(token) {
      const code = token && token.text ? String(token.text) : "";
      const lang = token && token.lang ? String(token.lang) : "";
      let highlighted;
      try {
        highlighted = lang && hljs.getLanguage(lang)
          ? hljs.highlight(code, { language: lang }).value
          : hljs.highlightAuto(code).value;
      } catch {
        highlighted = escapeHtml(code);
      }
      return `<pre><code class="hljs">${highlighted}</code></pre>`;
    },
    codespan(token) {
      return `<code>${escapeHtml(token && token.text ? token.text : "")}</code>`;
    },
  };

  if (window.marked && window.hljs) {
    marked.use({
      breaks: true,
      gfm: true,
      tokenizer: {
        html() {
          return undefined;
        },
        tag() {
          return undefined;
        },
      },
      renderer: markdownRenderer,
    });
  }

  function safeMarkedParse(text) {
    const raw = String(text || "");
    if (!window.marked) {
      return escapeHtml(raw);
    }
    return marked.parse(raw);
  }

  function appendMarkdown(parent, className, text) {
    const node = document.createElement("div");
    node.className = className;
    node.innerHTML = safeMarkedParse(text);
    parent.appendChild(node);
    return node;
  }

  function messageRole(entry) {
    return entry && entry.message ? String(entry.message.role || "").toLowerCase() : "";
  }

  function appendTextBlock(parent, className, text) {
    const node = document.createElement("div");
    node.className = className;
    node.textContent = text;
    parent.appendChild(node);
    return node;
  }

  function findToolResult(toolCallId) {
    for (const entry of entries) {
      const role = messageRole(entry);
      if (entry.type === "message" && role === "toolresult") {
        if (entry.message.toolCallId === toolCallId) {
          return entry.message;
        }
      }
    }
    return null;
  }

  function toolResultText(result) {
    if (!result) {
      return "";
    }
    const content = result.content;
    if (typeof content === "string") {
      return content;
    }
    if (Array.isArray(content)) {
      return content
        .filter((block) => block && block.type === "text")
        .map((block) => block.text || "")
        .join("\n");
    }
    return "";
  }

  function toolResultImages(result) {
    if (!result || !Array.isArray(result.content)) {
      return [];
    }
    return result.content.filter((block) => block && block.type === "image");
  }

  function formatToolArguments(args) {
    if (!args || typeof args !== "object") {
      return "";
    }
    try {
      return JSON.stringify(args, null, 2);
    } catch {
      return String(args);
    }
  }

  function renderToolCall(block) {
    const result = findToolResult(block.id);
    const isError = Boolean(result && result.isError);
    const card = document.createElement("div");
    card.className = `tool-execution ${result ? (isError ? "error" : "success") : "pending"}`;
    const rendered = renderedTools[block.id];
    if (rendered && (rendered.callHtml || rendered.resultHtmlCollapsed || rendered.resultHtmlExpanded)) {
      appendRenderedToolHtml(card, block, rendered, result);
      return card;
    }
    if (renderBuiltinToolCall(card, block, result)) {
      return card;
    }

    const title = document.createElement("div");
    title.className = "tool-header";
    title.textContent = formatToolCall(block.name, block.arguments);
    card.appendChild(title);

    const argsText = formatToolArguments(block.arguments);
    if (argsText) {
      appendTextBlock(card, "tool-args", argsText);
    }

    if (result) {
      appendToolResult(card, result);
    }

    return card;
  }

  function renderBuiltinToolCall(card, block, result) {
    const args = block.arguments && typeof block.arguments === "object" ? block.arguments : {};
    const name = block.name || "tool";
    switch (name) {
      case "bash": {
        appendToolCommand(card, stringArg(args.command) || "...");
        const output = toolResultText(result);
        if (output.trim()) {
          appendExpandableOutput(card, output, 5);
        }
        return true;
      }
      case "read": {
        const filePath = stringArg(args.file_path ?? args.path);
        const range = lineRange(args.offset, args.limit);
        appendToolHeader(card, "read", filePath || "", range);
        appendToolResultImages(card, result);
        const output = toolResultText(result);
        if (output.trim()) {
          appendExpandableOutput(card, output, 10);
        }
        return true;
      }
      case "write": {
        const filePath = stringArg(args.file_path ?? args.path);
        const content = stringArg(args.content);
        const lines = content ? content.split("\n").length : 0;
        appendToolHeader(card, "write", filePath || "", lines > 10 ? `(${lines} lines)` : "");
        if (content) {
          appendExpandableOutput(card, content, 10);
        }
        const output = toolResultText(result);
        if (output.trim()) {
          appendTextBlock(card, "tool-output", output.trim());
        }
        return true;
      }
      case "edit": {
        const filePath = stringArg(args.file_path ?? args.path);
        appendToolHeader(card, "edit", filePath || "", "");
        const diff = result && result.details && typeof result.details.diff === "string"
          ? result.details.diff
          : "";
        if (diff) {
          appendToolDiff(card, diff);
        } else {
          const output = toolResultText(result);
          if (output.trim()) {
            appendExpandableOutput(card, output, 10);
          }
        }
        return true;
      }
      case "ls": {
        const dirPath = stringArg(args.path) || ".";
        const limit = args.limit === undefined ? "" : `(limit ${String(args.limit)})`;
        appendToolHeader(card, "ls", dirPath, limit);
        const output = toolResultText(result);
        if (output.trim()) {
          appendExpandableOutput(card, output, 20);
        }
        return true;
      }
      default:
        return false;
    }
  }

  function appendToolCommand(parent, command) {
    const node = document.createElement("div");
    node.className = "tool-command";
    node.textContent = `$ ${command}`;
    parent.appendChild(node);
  }

  function appendToolHeader(parent, name, path, meta) {
    const header = document.createElement("div");
    header.className = "tool-header";
    const toolName = document.createElement("span");
    toolName.className = "tool-name";
    toolName.textContent = name;
    const toolPath = document.createElement("span");
    toolPath.className = "tool-path";
    toolPath.textContent = shortenPath(path);
    header.append(toolName, document.createTextNode(" "), toolPath);
    if (meta) {
      const lineCount = document.createElement("span");
      lineCount.className = "line-count";
      lineCount.textContent = meta;
      header.append(document.createTextNode(" "), lineCount);
    }
    parent.appendChild(header);
  }

  function appendToolResultImages(parent, result) {
    for (const image of toolResultImages(result)) {
      const img = document.createElement("img");
      img.className = "tool-image";
      img.src = `data:${escapeHtml(image.mimeType || "image/png")};base64,${escapeHtml(image.data || "")}`;
      parent.appendChild(img);
    }
  }

  function appendExpandableOutput(parent, text, maxLines) {
    const normalized = replaceTabs(text);
    const lines = normalized.split("\n");
    const output = document.createElement("div");
    output.className = "tool-output";
    if (lines.length > maxLines) {
      output.classList.add("expandable");
      output.addEventListener("click", () => {
        if (window.getSelection().toString()) {
          return;
        }
        output.classList.toggle("expanded");
      });
      const preview = document.createElement("div");
      preview.className = "output-preview";
      preview.textContent = lines.slice(0, maxLines).join("\n");
      const full = document.createElement("div");
      full.className = "output-full";
      full.textContent = normalized;
      output.append(preview, full);
    } else {
      output.textContent = normalized;
    }
    parent.appendChild(output);
  }

  function appendToolDiff(parent, diff) {
    const container = document.createElement("div");
    container.className = "tool-diff";
    for (const line of replaceTabs(diff).split("\n")) {
      const item = document.createElement("div");
      item.className = line.startsWith("+") ? "diff-added" : line.startsWith("-") ? "diff-removed" : "diff-context";
      item.textContent = line;
      container.appendChild(item);
    }
    parent.appendChild(container);
  }

  function formatExpandableOutput(text, maxLines) {
    const wrapper = document.createElement("div");
    appendExpandableOutput(wrapper, text, maxLines);
    return wrapper.firstElementChild;
  }

  function replaceTabs(text) {
    return String(text || "").replace(/\t/g, "  ");
  }

  function formatToolCall(name, args) {
    const params = args && typeof args === "object" ? args : {};
    switch (name) {
      case "read": {
        const path = shortenPath(String(params.path || params.file_path || ""));
        return `[read: ${path}${lineRange(params.offset, params.limit)}]`;
      }
      case "write":
        return `[write: ${shortenPath(String(params.path || params.file_path || ""))}]`;
      case "edit":
        return `[edit: ${shortenPath(String(params.path || params.file_path || ""))}]`;
      case "bash": {
        const rawCommand = String(params.command || "");
        const command = rawCommand.replace(/[\n\t]/g, " ").trim().slice(0, 50);
        return `[bash: ${command}${rawCommand.length > 50 ? "..." : ""}]`;
      }
      case "grep":
        return `[grep: /${params.pattern || ""}/ in ${shortenPath(String(params.path || "."))}]`;
      case "find":
        return `[find: ${params.pattern || ""} in ${shortenPath(String(params.path || "."))}]`;
      case "ls":
        return `[ls: ${shortenPath(String(params.path || "."))}]`;
      default: {
        const json = JSON.stringify(params);
        return `[${name || "tool"}: ${json.slice(0, 40)}${json.length > 40 ? "..." : ""}]`;
      }
    }
  }

  function firstToolCall(content) {
    if (!Array.isArray(content)) {
      return null;
    }
    return content.find((block) => block && block.type === "toolCall") || null;
  }

  function shortenPath(path) {
    const text = String(path || "");
    if (text.length <= 72) {
      return text;
    }
    return `...${text.slice(-69)}`;
  }

  function lineRange(offset, limit) {
    if (offset === undefined && limit === undefined) {
      return "";
    }
    const start = Number(offset || 1);
    if (limit === undefined) {
      return `:${start}`;
    }
    return `:${start}-${start + Number(limit) - 1}`;
  }

  function stringArg(value) {
    return typeof value === "string" ? value : "";
  }

  function appendRenderedToolHtml(card, block, rendered, result) {
    const title = document.createElement("div");
    title.className = rendered.callHtml ? "tool-header ansi-rendered" : "tool-header";
    if (rendered.callHtml) {
      title.innerHTML = rendered.callHtml;
    } else {
      title.textContent = formatToolCall(block.name, block.arguments);
    }
    card.appendChild(title);

    if (
      rendered.resultHtmlCollapsed &&
      rendered.resultHtmlExpanded &&
      rendered.resultHtmlCollapsed !== rendered.resultHtmlExpanded
    ) {
      const output = document.createElement("div");
      output.className = "tool-output expandable ansi-rendered";
      output.addEventListener("click", () => {
        if (window.getSelection().toString()) {
          return;
        }
        output.classList.toggle("expanded");
      });
      const preview = document.createElement("div");
      preview.className = "output-preview";
      preview.innerHTML = rendered.resultHtmlCollapsed;
      const full = document.createElement("div");
      full.className = "output-full";
      full.innerHTML = rendered.resultHtmlExpanded;
      output.append(preview, full);
      card.appendChild(output);
      return;
    }

    if (rendered.resultHtmlExpanded) {
      const output = document.createElement("div");
      output.className = "tool-output ansi-rendered";
      output.innerHTML = rendered.resultHtmlExpanded;
      card.appendChild(output);
      return;
    }

    if (result) {
      appendToolResult(card, result);
    }
  }

  window.downloadSessionJson = function() {
    const lines = [];
    if (header) {
      lines.push(JSON.stringify({ type: "header", ...header }));
    }
    for (const entry of entries) {
      lines.push(JSON.stringify(entry));
    }

    const blob = new Blob([lines.join("\n")], { type: "application/x-ndjson" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${header.id || "session"}.jsonl`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  function appendToolResult(parent, result) {
    for (const image of toolResultImages(result)) {
      const img = document.createElement("img");
      img.className = "tool-image";
      img.src = `data:${escapeHtml(image.mimeType || "image/png")};base64,${escapeHtml(image.data || "")}`;
      parent.appendChild(img);
    }

    const text = toolResultText(result);
    if (text.trim()) {
      appendTextBlock(parent, "tool-output expandable", text);
    }
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

  function renderEntryBody(body, entry) {
    const role = messageRole(entry);
    if (entry.type === "message" && role === "toolresult") {
      return;
    }

    if (entry.type === "message" && entry.message && role === "user") {
      const content = entry.message.content;
      const text = messageText(content);
      const skillBlock = parseSkillBlock(text);
      if (skillBlock) {
        renderSkillUserEntry(body, skillBlock, messageImages(content));
        return;
      }
      for (const image of messageImages(content)) {
        const img = document.createElement("img");
        img.className = "message-image";
        img.src = `data:${escapeHtml(image.mimeType || "image/png")};base64,${escapeHtml(image.data || "")}`;
        body.appendChild(img);
      }
      if (text.trim()) {
        appendMarkdown(body, "markdown-content", text);
        return;
      }
    }

    if (entry.type === "message" && entry.message && role === "assistant") {
      const content = entry.message.content;
      if (Array.isArray(content)) {
        for (const block of content) {
          if (block && block.type === "text" && String(block.text || "").trim()) {
            appendMarkdown(body, "assistant-text markdown-content", block.text);
          } else if (block && block.type === "thinking" && String(block.thinking || "").trim()) {
            const thinking = document.createElement("div");
            thinking.className = "thinking-block";
            const thinkingText = document.createElement("div");
            thinkingText.className = "thinking-text";
            thinkingText.textContent = block.thinking;
            const thinkingCollapsed = document.createElement("div");
            thinkingCollapsed.className = "thinking-collapsed";
            thinkingCollapsed.textContent = "Thinking ...";
            thinking.append(thinkingText, thinkingCollapsed);
            body.appendChild(thinking);
          } else if (block && block.type === "toolCall") {
            body.appendChild(renderToolCall(block));
          }
        }
        return;
      }
      const text = messageText(content);
      if (text.trim()) {
        appendMarkdown(body, "assistant-text markdown-content", text);
        return;
      }
    }

    if (entry.type === "compaction" || entry.type === "branch_summary" || entry.type === "custom_message") {
      appendMarkdown(body, "markdown-content", entryText(entry));
      return;
    }

    body.textContent = entryText(entry);
  }

  function messageText(content) {
    if (typeof content === "string") {
      return content;
    }
    if (Array.isArray(content)) {
      return content
        .filter((block) => block && block.type === "text")
        .map((block) => block.text || "")
        .join("\n");
    }
    return "";
  }

  function messageImages(content) {
    if (!Array.isArray(content)) {
      return [];
    }
    return content.filter((block) => block && block.type === "image");
  }

  function parseSkillBlock(text) {
    const match = String(text).match(/^<skill name="([^"]+)" location="([^"]+)">\n([\s\S]*?)\n<\/skill>(?:\n\n([\s\S]+))?$/);
    if (!match) {
      return null;
    }
    return {
      name: match[1],
      location: match[2],
      content: match[3],
      userMessage: match[4] ? match[4].trim() : undefined,
    };
  }

  function renderSkillUserEntry(body, skillBlock, images) {
    body.classList.add("skill-user-entry");

    const skill = document.createElement("div");
    skill.className = "skill-invocation";
    skill.addEventListener("click", () => {
      if (window.getSelection().toString()) {
        return;
      }
      skill.classList.toggle("expanded");
    });

    const label = document.createElement("div");
    label.className = "skill-invocation-label";
    label.textContent = `[skill] ${skillBlock.name}`;
    const collapsed = document.createElement("div");
    collapsed.className = "skill-invocation-collapsed";
    collapsed.textContent = `${skillBlock.name} (click to expand)`;
    const content = document.createElement("div");
    content.className = "skill-invocation-content";
    content.innerHTML = safeMarkedParse(skillBlock.content);
    skill.append(label, collapsed, content);
    body.appendChild(skill);

    const hasUserContent = Boolean(skillBlock.userMessage) || images.length > 0;
    if (!hasUserContent) {
      return;
    }

    const user = document.createElement("div");
    user.className = "user-message";
    for (const image of images) {
      const img = document.createElement("img");
      img.className = "message-image";
      img.src = `data:${escapeHtml(image.mimeType || "image/png")};base64,${escapeHtml(image.data || "")}`;
      user.appendChild(img);
    }
    if (skillBlock.userMessage) {
      appendMarkdown(user, "markdown-content", skillBlock.userMessage);
    }
    body.appendChild(user);
  }
})();
