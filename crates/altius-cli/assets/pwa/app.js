/* Altius Fleet PWA thin client - vanilla JS, zero build. */
(() => {
  const THEME_KEY = "altius-fleet-theme";
  const TOKEN_KEY = "altius-fleet-token";
  const THEME_CYCLE = ["system", "light", "dark"];

  const params = new URLSearchParams(location.search);
  const apiBase = (params.get("api") || location.origin).replace(/\/$/, "");

  // Bearer token: ?token= wins, then sessionStorage. Remove credentials
  // from the visible URL immediately so they do not remain in browser
  // history or leak via copied links.
  if (params.get("token")) {
    sessionStorage.setItem(TOKEN_KEY, params.get("token"));
    const cleanUrl = new URL(location.href);
    cleanUrl.searchParams.delete("token");
    history.replaceState(null, "", cleanUrl);
  }
  let authToken = sessionStorage.getItem(TOKEN_KEY) || "";

  const els = {
    prompt: document.getElementById("prompt"),
    send: document.getElementById("send"),
    sendIcon: document.getElementById("send-icon"),
    refresh: document.getElementById("refresh"),
    error: document.getElementById("error"),
    runs: document.getElementById("runs"),
    detail: document.getElementById("detail"),
    detailId: document.getElementById("detail-id"),
    detailStatus: document.getElementById("detail-status"),
    detailBody: document.getElementById("detail-body"),
    approval: document.getElementById("approval"),
    approvalMsg: document.getElementById("approval-msg"),
    approve: document.getElementById("approve"),
    cancel: document.getElementById("cancel"),
    agentPills: document.getElementById("agent-pills"),
    agentEyebrow: document.getElementById("agent-eyebrow"),
    themeToggle: document.getElementById("theme-toggle"),
    themeIcon: document.getElementById("theme-icon"),
    navNew: document.getElementById("nav-new"),
    navDispatch: document.getElementById("nav-dispatch"),
    navRuns: document.getElementById("nav-runs"),
    mainInner: document.getElementById("main-inner"),
    sidebarHistory: document.getElementById("sidebar-history"),
    historyList: document.getElementById("history-list"),
    sidebarMeta: document.querySelector(".sidebar-meta"),
  };

  let selectedId = null;
  let pollTimer = null;
  let eventSource = null;
  let selectedAgent = "browser";
  const known = new Map();

  function escapeHtml(value) {
    return String(value)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;");
  }

  function showError(message) {
    if (!message) {
      els.error.hidden = true;
      els.error.textContent = "";
      return;
    }
    els.error.hidden = false;
    els.error.textContent = message;
  }

  function authHeaders(extra = {}) {
    const headers = { "content-type": "application/json", ...extra };
    if (authToken) headers.authorization = `Bearer ${authToken}`;
    return headers;
  }

  async function api(path, options = {}) {
    const response = await fetch(`${apiBase}${path}`, {
      headers: authHeaders(options.headers || {}),
      ...options,
    });
    const text = await response.text();
    let body = null;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = { raw: text };
    }
    if (!response.ok) {
      const msg =
        (body && (body.error || body.message || body.detail)) ||
        `${response.status} ${response.statusText}`;
      throw new Error(typeof msg === "string" ? msg : JSON.stringify(msg));
    }
    return body;
  }

  function flattenParts(messages) {
    if (!Array.isArray(messages)) return "";
    return messages
      .flatMap((m) => (m.parts || []).map((p) => p.content || ""))
      .filter(Boolean)
      .join("\n");
  }

  function relativeTime(value) {
    if (!value) return "";
    const then = new Date(value).getTime();
    if (Number.isNaN(then)) return "";
    const seconds = Math.round((Date.now() - then) / 1000);
    if (seconds < 45) return "just now";
    const units = [
      ["d", 86400],
      ["h", 3600],
      ["m", 60],
    ];
    for (const [label, span] of units) {
      const amount = Math.floor(seconds / span);
      if (amount >= 1) return `${amount}${label} ago`;
    }
    return "just now";
  }

  function dateGroupLabel(value) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "Earlier";
    return date.toLocaleDateString(undefined, {
      weekday: "long",
      month: "long",
      day: "numeric",
    });
  }

  function badgeClass(status) {
    const knownStatus = [
      "created",
      "in-progress",
      "awaiting",
      "completed",
      "failed",
      "cancelled",
    ];
    return knownStatus.includes(status) ? `badge ${status}` : "badge";
  }

  function setAgent(agent) {
    selectedAgent = agent;
    if (els.agentEyebrow) els.agentEyebrow.textContent = agent;
    for (const btn of els.agentPills.querySelectorAll("[data-agent]")) {
      const pressed = btn.dataset.agent === agent;
      btn.setAttribute("aria-pressed", pressed ? "true" : "false");
    }
    if (els.prompt) {
      if (agent === "security") {
        els.prompt.placeholder =
          "@Security audit this program for missing signer checks";
      } else if (agent === "browser") {
        els.prompt.placeholder =
          "@Browser open https://example.com and summarize the title";
      } else {
        els.prompt.placeholder = "Ask the Altius fleet…";
      }
    }
  }

  function updateHome() {
    if (els.mainInner) els.mainInner.classList.toggle("home", !selectedId);
  }

  function setView(view) {
    document.body.dataset.mobileView = view;
    for (const btn of [els.navDispatch, els.navRuns]) {
      if (!btn) continue;
      const active = btn.dataset.view === view;
      btn.classList.toggle("active", active);
      if (active) btn.setAttribute("aria-current", "page");
      else btn.removeAttribute("aria-current");
    }
  }

  function readStoredTheme() {
    const stored = localStorage.getItem(THEME_KEY);
    return THEME_CYCLE.includes(stored) ? stored : "system";
  }

  function applyTheme(mode) {
    const root = document.documentElement;
    if (mode === "system") root.removeAttribute("data-theme");
    else root.dataset.theme = mode;

    const icons = { system: "◐", light: "☀", dark: "☾" };
    els.themeIcon.textContent = icons[mode] || "◐";
    els.themeToggle.title = `Theme: ${mode} (click to cycle)`;
    els.themeToggle.setAttribute(
      "aria-label",
      `Color theme: ${mode}. Click to cycle.`
    );
  }

  function initTheme() {
    applyTheme(readStoredTheme());
  }

  function cycleTheme() {
    const current = readStoredTheme();
    const next = THEME_CYCLE[(THEME_CYCLE.indexOf(current) + 1) % THEME_CYCLE.length];
    localStorage.setItem(THEME_KEY, next);
    applyTheme(next);
  }

  function openRun(id) {
    selectRun(id);
    setView("dispatch");
  }

  function newRun() {
    selectedId = null;
    clearInterval(pollTimer);
    pollTimer = null;
    stopEvents();
    renderDetail(null);
    updateHome();
    els.prompt.value = "";
    setView("dispatch");
    els.prompt.focus();
    const runs = [...known.values()].sort(
      (a, b) => new Date(b.created_at) - new Date(a.created_at)
    );
    renderRuns(runs);
  }

  function renderHistory(runs) {
    if (!els.historyList || !els.sidebarHistory) return;
    els.historyList.innerHTML = "";
    if (!runs.length) {
      els.sidebarHistory.hidden = true;
      return;
    }
    els.sidebarHistory.hidden = false;
    for (const run of runs.slice(0, 8)) {
      const li = document.createElement("li");
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className =
        "history-item" + (run.run_id === selectedId ? " active" : "");
      btn.textContent = flattenParts(run.input).slice(0, 60) || "(empty)";
      btn.title = flattenParts(run.input).slice(0, 200);
      btn.addEventListener("click", () => openRun(run.run_id));
      li.appendChild(btn);
      els.historyList.appendChild(li);
    }
  }

  function renderRuns(runs) {
    renderHistory(runs);
    els.runs.innerHTML = "";
    if (!runs.length) {
      const empty = document.createElement("li");
      empty.className = "runs-empty";
      empty.textContent = "No runs yet. Dispatch one to get started.";
      els.runs.appendChild(empty);
      return;
    }
    let lastGroup = null;
    for (const run of runs) {
      known.set(run.run_id, run);
      const group = dateGroupLabel(run.created_at);
      if (group !== lastGroup) {
        lastGroup = group;
        const header = document.createElement("li");
        header.className = "runs-group";
        header.textContent = group;
        els.runs.appendChild(header);
      }
      const li = document.createElement("li");
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "list-row" + (run.run_id === selectedId ? " active" : "");
      const preview = flattenParts(run.input).slice(0, 80) || "(empty)";
      const when = relativeTime(run.created_at);
      btn.innerHTML = `
        <div class="list-row-title">
          <span class="${badgeClass(run.status)}">${escapeHtml(run.status)}</span>
          <span>${escapeHtml(run.agent_name)}</span>
          ${when ? `<span class="list-row-time">${escapeHtml(when)}</span>` : ""}
        </div>
        <div class="list-row-preview">${escapeHtml(preview)}</div>
        <div class="list-row-meta">${escapeHtml(run.run_id)}</div>`;
      btn.addEventListener("click", () => openRun(run.run_id));
      li.appendChild(btn);
      els.runs.appendChild(li);
    }
  }

  function renderDetail(run) {
    updateHome();
    if (!run) {
      els.detail.hidden = true;
      return;
    }
    els.detail.hidden = false;
    els.detailId.textContent = run.run_id;
    els.detailStatus.textContent = run.status;
    els.detailStatus.className = badgeClass(run.status);
    const input = flattenParts(run.input);
    const output = flattenParts(run.output);
    const error = run.error || "";
    els.detailBody.textContent = [
      `agent    ${run.agent_name}`,
      `status   ${run.status}`,
      "",
      "INPUT",
      input || "(none)",
      "",
      "OUTPUT",
      output || "(none)",
      error ? `\nERROR\n${error}` : "",
    ].join("\n");

    els.approval.hidden = run.status !== "awaiting";
  }

  function stopEvents() {
    if (eventSource) {
      eventSource.close();
      eventSource = null;
    }
  }

  function maybeEvents(run) {
    stopEvents();
    if (!run) return;
    if (run.status !== "in-progress" && run.status !== "created") return;
    if (typeof EventSource === "undefined") return;
    try {
      const url = new URL(`${apiBase}/runs/${run.run_id}/events`);
      if (authToken) url.searchParams.set("token", authToken);
      eventSource = new EventSource(url.toString());
      eventSource.addEventListener("run", (ev) => {
        try {
          const next = JSON.parse(ev.data);
          known.set(next.run_id, next);
          renderDetail(next);
          const runs = [...known.values()].sort(
            (a, b) => new Date(b.created_at) - new Date(a.created_at)
          );
          renderRuns(runs);
          if (
            next.status !== "in-progress" &&
            next.status !== "created"
          ) {
            stopEvents();
          }
        } catch {
          /* ignore malformed */
        }
      });
      eventSource.onerror = () => {
        stopEvents();
        maybePoll(run);
      };
    } catch {
      maybePoll(run);
    }
  }

  async function refreshRuns() {
    showError("");
    try {
      const runs = await api("/runs");
      renderRuns(Array.isArray(runs) ? runs : []);
      if (selectedId) {
        const current =
          known.get(selectedId) || (await api(`/runs/${selectedId}`));
        renderDetail(current);
        maybeEvents(current);
        maybePoll(current);
      }
    } catch (err) {
      showError(err.message || String(err));
    }
  }

  function maybePoll(run) {
    clearInterval(pollTimer);
    pollTimer = null;
    if (!run) return;
    if (eventSource) return;
    if (run.status === "in-progress" || run.status === "created") {
      pollTimer = setInterval(() => selectRun(run.run_id, true), 1500);
    }
  }

  async function selectRun(id, quiet = false) {
    selectedId = id;
    try {
      const run = await api(`/runs/${id}`);
      known.set(id, run);
      if (!quiet) showError("");
      renderDetail(run);
      const runs = [...known.values()].sort(
        (a, b) => new Date(b.created_at) - new Date(a.created_at)
      );
      renderRuns(runs);
      maybeEvents(run);
      maybePoll(run);
    } catch (err) {
      if (!quiet) showError(err.message || String(err));
    }
  }

  /** Parse leading slash skill: /scan /browser /audit /pay */
  function resolveSkill(prompt) {
    const m = prompt.match(/^\/(scan|browser|audit|pay)\b\s*/i);
    if (!m) return { agent: selectedAgent, prompt };
    const skill = m[1].toLowerCase();
    const rest = prompt.slice(m[0].length).trim() || prompt;
    const map = {
      scan: "security",
      audit: "security",
      browser: "browser",
      pay: "altius",
    };
    return { agent: map[skill] || selectedAgent, prompt: rest };
  }

  async function sendRun() {
    const raw = els.prompt.value.trim();
    if (!raw) {
      showError("Enter a prompt before sending.");
      return;
    }
    const { agent, prompt } = resolveSkill(raw);
    if (agent !== selectedAgent) setAgent(agent);

    els.send.disabled = true;
    if (els.sendIcon) els.sendIcon.textContent = "…";
    showError("");
    try {
      const run = await api("/runs", {
        method: "POST",
        body: JSON.stringify({
          agent_name: agent,
          input: [
            {
              role: "user",
              parts: [{ content_type: "text/plain", content: prompt }],
            },
          ],
        }),
      });
      known.set(run.run_id, run);
      selectedId = run.run_id;
      await refreshRuns();
      renderDetail(run);
      maybeEvents(run);
      maybePoll(run);
      setView("dispatch");
    } catch (err) {
      showError(err.message || String(err));
    } finally {
      els.send.disabled = false;
      if (els.sendIcon) els.sendIcon.textContent = "↑";
    }
  }

  async function resumeRun() {
    if (!selectedId) return;
    els.approve.disabled = true;
    try {
      const message = els.approvalMsg.value.trim();
      const body = message
        ? {
            message: {
              role: "user",
              parts: [{ content_type: "text/plain", content: message }],
            },
          }
        : {};
      const run = await api(`/runs/${selectedId}`, {
        method: "POST",
        body: JSON.stringify(body),
      });
      known.set(run.run_id, run);
      renderDetail(run);
      maybeEvents(run);
      maybePoll(run);
      await refreshRuns();
    } catch (err) {
      showError(err.message || String(err));
    } finally {
      els.approve.disabled = false;
    }
  }

  async function cancelRun() {
    if (!selectedId) return;
    els.cancel.disabled = true;
    try {
      const run = await api(`/runs/${selectedId}/cancel`, {
        method: "POST",
        body: "{}",
      });
      known.set(run.run_id, run);
      renderDetail(run);
      stopEvents();
      maybePoll(run);
      await refreshRuns();
    } catch (err) {
      showError(err.message || String(err));
    } finally {
      els.cancel.disabled = false;
    }
  }

  els.agentPills.addEventListener("click", (event) => {
    const btn = event.target.closest("[data-agent]");
    if (!btn) return;
    setAgent(btn.dataset.agent);
  });

  if (els.navNew) els.navNew.addEventListener("click", newRun);
  els.navDispatch.addEventListener("click", () => setView("dispatch"));
  els.navRuns.addEventListener("click", () => setView("runs"));
  els.themeToggle.addEventListener("click", cycleTheme);
  els.send.addEventListener("click", sendRun);
  if (els.refresh) els.refresh.addEventListener("click", refreshRuns);
  els.approve.addEventListener("click", resumeRun);
  els.cancel.addEventListener("click", cancelRun);

  els.prompt.addEventListener("keydown", (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      sendRun();
    }
  });

  if (els.sidebarMeta) {
    els.sidebarMeta.textContent = authToken
      ? "localhost · bearer auth"
      : "localhost · no auth";
  }

  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("./sw.js").catch(() => {
      /* optional */
    });
  }

  initTheme();
  setAgent("browser");
  setView("dispatch");
  updateHome();
  refreshRuns();
})();
