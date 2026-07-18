/* Altius Fleet PWA thin client — vanilla JS, zero build. */
(() => {
  const apiBase = (() => {
    // Same-origin when served from /app/; allow override via query.
    const params = new URLSearchParams(location.search);
    return (params.get("api") || location.origin).replace(/\/$/, "");
  })();

  const els = {
    agent: document.getElementById("agent"),
    prompt: document.getElementById("prompt"),
    send: document.getElementById("send"),
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
  };

  let selectedId = null;
  let pollTimer = null;
  const known = new Map();

  function showError(message) {
    if (!message) {
      els.error.hidden = true;
      els.error.textContent = "";
      return;
    }
    els.error.hidden = false;
    els.error.textContent = message;
  }

  async function api(path, options = {}) {
    const response = await fetch(`${apiBase}${path}`, {
      headers: { "content-type": "application/json", ...(options.headers || {}) },
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

  function renderRuns(runs) {
    els.runs.innerHTML = "";
    if (!runs.length) {
      els.runs.innerHTML = `<li style="cursor:default;color:var(--muted)">No runs yet</li>`;
      return;
    }
    for (const run of runs) {
      known.set(run.run_id, run);
      const li = document.createElement("li");
      if (run.run_id === selectedId) li.classList.add("active");
      const preview = flattenParts(run.input).slice(0, 80) || "(empty)";
      li.innerHTML = `<div><span class="status ${run.status}">${run.status}</span>
        <strong style="margin-left:0.4rem">${run.agent_name}</strong></div>
        <div style="color:var(--muted);font-size:0.85rem;margin-top:0.25rem">${escapeHtml(preview)}</div>
        <div style="color:var(--muted);font-size:0.75rem;margin-top:0.25rem">${run.run_id}</div>`;
      li.addEventListener("click", () => selectRun(run.run_id));
      els.runs.appendChild(li);
    }
  }

  function escapeHtml(value) {
    return String(value)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;");
  }

  function renderDetail(run) {
    if (!run) {
      els.detail.hidden = true;
      return;
    }
    els.detail.hidden = false;
    els.detailId.textContent = run.run_id;
    els.detailStatus.textContent = run.status;
    els.detailStatus.className = `status ${run.status}`;
    const input = flattenParts(run.input);
    const output = flattenParts(run.output);
    const error = run.error || "";
    els.detailBody.textContent = [
      `agent: ${run.agent_name}`,
      `status: ${run.status}`,
      "",
      "— input —",
      input || "(none)",
      "",
      "— output —",
      output || "(none)",
      error ? `\n— error —\n${error}` : "",
    ].join("\n");

    const awaiting = run.status === "awaiting";
    els.approval.hidden = !awaiting;
  }

  async function refreshRuns() {
    showError("");
    try {
      const runs = await api("/runs");
      renderRuns(Array.isArray(runs) ? runs : []);
      if (selectedId) {
        const current = known.get(selectedId) || (await api(`/runs/${selectedId}`));
        renderDetail(current);
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
      // Refresh list highlighting without another network round-trip when possible.
      const runs = [...known.values()].sort(
        (a, b) => new Date(b.created_at) - new Date(a.created_at)
      );
      renderRuns(runs);
      maybePoll(run);
    } catch (err) {
      if (!quiet) showError(err.message || String(err));
    }
  }

  async function sendRun() {
    const prompt = els.prompt.value.trim();
    if (!prompt) {
      showError("Prompt is required");
      return;
    }
    els.send.disabled = true;
    showError("");
    try {
      const run = await api("/runs", {
        method: "POST",
        body: JSON.stringify({
          agent_name: els.agent.value,
          input: [{ role: "user", parts: [{ content_type: "text/plain", content: prompt }] }],
        }),
      });
      known.set(run.run_id, run);
      selectedId = run.run_id;
      await refreshRuns();
      renderDetail(run);
      maybePoll(run);
    } catch (err) {
      showError(err.message || String(err));
    } finally {
      els.send.disabled = false;
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
      const run = await api(`/runs/${selectedId}/cancel`, { method: "POST", body: "{}" });
      known.set(run.run_id, run);
      renderDetail(run);
      maybePoll(run);
      await refreshRuns();
    } catch (err) {
      showError(err.message || String(err));
    } finally {
      els.cancel.disabled = false;
    }
  }

  els.send.addEventListener("click", sendRun);
  els.refresh.addEventListener("click", refreshRuns);
  els.approve.addEventListener("click", resumeRun);
  els.cancel.addEventListener("click", cancelRun);

  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("./sw.js").catch(() => {
      /* optional */
    });
  }

  refreshRuns();
})();
