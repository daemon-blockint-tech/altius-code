// Thin BeeAI ACP client mirroring the wire shape served by
// `altius fleet serve` (crates/altius-protocol/src/beeacp). Kept dependency
// free (uses the extension host's global fetch) so this compiles without
// pulling in an HTTP library.

export type RunStatus =
  | "created"
  | "in-progress"
  | "awaiting"
  | "completed"
  | "failed"
  | "cancelled";

export interface MessagePart {
  content_type: string;
  content: string;
}

export interface Message {
  role: string;
  parts: MessagePart[];
}

export interface LamportDelta {
  account: string;
  delta_lamports: string;
}

export interface TransactionPreview {
  action_summary?: string;
  lamport_deltas?: LamportDelta[];
  invoked_programs?: string[];
  compute_units_consumed?: number;
  compute_unit_limit?: number;
}

export interface RunApproval {
  summary: string;
  reason?: string;
  node?: string;
  kind: "generic" | "transaction";
  transaction?: TransactionPreview;
}

export interface Run {
  run_id: string;
  agent_name: string;
  status: RunStatus;
  input: Message[];
  output: Message[];
  error?: string;
  approval?: RunApproval;
  created_at: string;
  finished_at?: string;
}

export interface ApprovalDecision {
  approved: boolean;
  note?: string;
}

export interface ResumeRunRequest {
  message?: Message;
  decision?: ApprovalDecision;
}

export interface BeeAcpClientOptions {
  baseUrl: string;
  token?: string;
}

export function userTextMessage(text: string): Message {
  return { role: "user", parts: [{ content_type: "text/plain", content: text }] };
}

export class BeeAcpClient {
  private readonly baseUrl: string;
  private readonly token?: string;

  constructor(options: BeeAcpClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.token = options.token;
  }

  private authHeaders(extra?: Record<string, string>): Record<string, string> {
    const headers: Record<string, string> = { "content-type": "application/json", ...extra };
    if (this.token) headers.authorization = `Bearer ${this.token}`;
    return headers;
  }

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...init,
      headers: this.authHeaders(init?.headers as Record<string, string> | undefined),
    });
    const text = await response.text();
    let body: unknown = null;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = { raw: text };
    }
    if (!response.ok) {
      const message = extractErrorMessage(body) ?? `${response.status} ${response.statusText}`;
      throw new Error(message);
    }
    return body as T;
  }

  listRuns(): Promise<Run[]> {
    return this.request<Run[]>("/runs");
  }

  getRun(id: string): Promise<Run> {
    return this.request<Run>(`/runs/${encodeURIComponent(id)}`);
  }

  createRun(agentName: string, input: Message[]): Promise<Run> {
    return this.request<Run>("/runs", {
      method: "POST",
      body: JSON.stringify({ agent_name: agentName, input }),
    });
  }

  resumeRun(id: string, body: ResumeRunRequest = {}): Promise<Run> {
    return this.request<Run>(`/runs/${encodeURIComponent(id)}`, {
      method: "POST",
      body: JSON.stringify(body),
    });
  }

  cancelRun(id: string): Promise<Run> {
    return this.request<Run>(`/runs/${encodeURIComponent(id)}/cancel`, {
      method: "POST",
      body: "{}",
    });
  }
}

function extractErrorMessage(body: unknown): string | undefined {
  if (!body || typeof body !== "object") return undefined;
  const record = body as Record<string, unknown>;
  const error = record.error;
  if (error && typeof error === "object") {
    const message = (error as Record<string, unknown>).message;
    if (typeof message === "string") return message;
  }
  if (typeof error === "string") return error;
  if (typeof record.message === "string") return record.message;
  if (typeof record.detail === "string") return record.detail;
  return undefined;
}
