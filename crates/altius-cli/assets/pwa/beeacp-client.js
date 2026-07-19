/**
 * Hand-typed BeeACP wire client (zero build). Matches OpenAPI at `/openapi.json`.
 */

/** @typedef {'created'|'in-progress'|'awaiting'|'completed'|'failed'|'cancelled'} RunStatus */
/** @typedef {'generic'|'transaction'} ApprovalKind */

/**
 * @typedef {Object} LamportDelta
 * @property {string} account
 * @property {string} delta_lamports Signed decimal string
 */

/**
 * @typedef {Object} TransactionPreview
 * @property {string} [action_summary]
 * @property {LamportDelta[]} [lamport_deltas]
 * @property {string[]} [invoked_programs]
 * @property {number} [compute_units_consumed]
 * @property {number} [compute_unit_limit]
 */

/**
 * @typedef {Object} RunApproval
 * @property {string} summary Human-readable headline
 * @property {string} [reason]
 * @property {string} [node]
 * @property {ApprovalKind} kind
 * @property {TransactionPreview} [transaction]
 */

/**
 * @typedef {Object} MessagePart
 * @property {string} content_type
 * @property {string} content
 */

/**
 * @typedef {Object} Message
 * @property {string} role
 * @property {MessagePart[]} parts
 */

/**
 * @typedef {Object} Run
 * @property {string} run_id
 * @property {string} agent_name
 * @property {RunStatus} status
 * @property {Message[]} input
 * @property {Message[]} output
 * @property {string} [error]
 * @property {RunApproval} [approval] Present when status is `awaiting`
 * @property {string} created_at ISO-8601
 * @property {string} [finished_at]
 */

/**
 * @typedef {Object} ApprovalDecision
 * @property {boolean} approved
 * @property {string} [note]
 */

/**
 * @typedef {Object} ResumeRunRequest
 * @property {Message} [message]
 * @property {ApprovalDecision} [decision]
 */

/**
 * Thin typed client for BeeACP `/runs*` (see `/openapi.json`).
 * @param {{ baseUrl: string, token?: string }} options
 */
function createBeeAcpClient(options) {
  const baseUrl = options.baseUrl.replace(/\/$/, "");
  const token = options.token || "";

  function authHeaders(extra = {}) {
    const headers = { "content-type": "application/json", ...extra };
    if (token) headers.authorization = `Bearer ${token}`;
    return headers;
  }

  async function request(path, options = {}) {
    const response = await fetch(`${baseUrl}${path}`, {
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
        (body &&
          (body.error?.message || body.error || body.message || body.detail)) ||
        `${response.status} ${response.statusText}`;
      throw new Error(typeof msg === "string" ? msg : JSON.stringify(msg));
    }
    return body;
  }

  return {
    request,
    listRuns: () => request("/runs"),
    getRun: (id) => request(`/runs/${id}`),
    createRun: (agentName, input) =>
      request("/runs", {
        method: "POST",
        body: JSON.stringify({ agent_name: agentName, input }),
      }),
    resumeRun: (id, body = {}) =>
      request(`/runs/${id}`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
    cancelRun: (id) =>
      request(`/runs/${id}/cancel`, { method: "POST", body: "{}" }),
    subscribeRunEvents(id, onRun) {
      if (typeof EventSource === "undefined") return null;
      const url = new URL(`${baseUrl}/runs/${id}/events`);
      if (token) url.searchParams.set("token", token);
      const source = new EventSource(url.toString());
      source.addEventListener("run", (ev) => {
        try {
          onRun(JSON.parse(ev.data));
        } catch {
          /* ignore malformed */
        }
      });
      return source;
    },
  };
}

/** @param {RunApproval|undefined|null} approval */
function formatApprovalSummary(approval) {
  if (!approval) return "Approval required";
  if (approval.transaction?.action_summary) {
    return approval.transaction.action_summary;
  }
  return approval.summary || "Approval required";
}

/** @param {RunApproval|undefined|null} approval */
function formatApprovalDetail(approval) {
  if (!approval) return "";
  const lines = [];
  if (approval.reason && approval.reason !== approval.summary) {
    lines.push(approval.reason);
  }
  if (approval.node) lines.push(`node: ${approval.node}`);
  const tx = approval.transaction;
  if (tx?.invoked_programs?.length) {
    lines.push(`programs: ${tx.invoked_programs.join(", ")}`);
  }
  if (tx?.lamport_deltas?.length) {
    for (const delta of tx.lamport_deltas) {
      lines.push(`${delta.account}: ${delta.delta_lamports} lamports`);
    }
  }
  return lines.join("\n");
}
