// Mirrors the JSON shape of altius_findings::{ScanReport, Finding} —
// see crates/altius-findings/src/{report,finding,severity}.rs. Kept as a
// plain structural type so `altius scan --format json` output can be
// JSON.parse()'d straight into this shape.

export type Severity = "info" | "low" | "medium" | "high" | "critical";
export type Confidence = "low" | "medium" | "high";

export interface FindingLocation {
  file: string;
  start_line?: number;
  end_line?: number;
  start_column?: number;
  end_column?: number;
  snippet?: string;
}

export interface Finding {
  id: string;
  chain: string;
  pattern_id: string;
  severity: Severity;
  confidence: Confidence;
  title: string;
  description: string;
  location: FindingLocation;
  attack_scenario?: string;
  recommendation?: string;
  tool: string;
  fingerprint: string;
}

export interface ScanReport {
  target: string;
  chain?: string;
  findings: Finding[];
  scanners: string[];
  notes?: string;
}

const SEVERITY_ORDER: Record<Severity, number> = {
  critical: 0,
  high: 1,
  medium: 2,
  low: 3,
  info: 4,
};

export function compareBySeverity(a: Finding, b: Finding): number {
  return SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity];
}
