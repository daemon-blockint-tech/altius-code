export type RunStatus =
  | 'created'
  | 'in-progress'
  | 'awaiting'
  | 'completed'
  | 'failed'
  | 'cancelled'

export interface MessagePart {
  content_type: string
  content: string
}

export interface Message {
  role: string
  parts: MessagePart[]
}

export interface LamportDelta {
  account: string
  delta_lamports: string
}

export interface TransactionPreview {
  action_summary?: string
  lamport_deltas?: LamportDelta[]
  invoked_programs?: string[]
  compute_units_consumed?: number
  compute_unit_limit?: number
}

export interface RunApproval {
  summary: string
  reason?: string
  node?: string
  kind: 'generic' | 'transaction'
  transaction?: TransactionPreview
}

export interface Run {
  run_id: string
  agent_name: string
  status: RunStatus
  input: Message[]
  output: Message[]
  error?: string
  approval?: RunApproval
  created_at: string
  finished_at?: string
}

export interface CreateRunRequest {
  agent_name: string
  input: Message[]
}

export interface ResumeRunRequest {
  message?: Message
  decision?: { approved: boolean; note?: string }
}

export interface SseEvent {
  type: 'run'
  run: Run
}

export interface AgentSkill {
  id: string
  name: string
  description: string
  tags: string[]
  examples: string[]
}

export interface AgentCard {
  protocol_version: string
  name: string
  description: string
  url: string
  version: string
  skills: AgentSkill[]
}
