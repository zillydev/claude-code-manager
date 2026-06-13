export interface SessionSummary {
  id: string;
  file_path: string;
  project_label: string;
  cwd: string | null;
  git_branch: string | null;
  title: string;
  mtime: { secs_since_epoch: number; nanos_since_epoch: number };
  total_tokens: number;
}

export type SortField = "updated" | "project" | "title" | "tokens";
export type SortDir = "asc" | "desc";
export type DateRange = "all" | "today" | "week" | "month";

export interface RenderedSession {
  project_label: string;
  id: string;
  source_path: string;
  title: string;
  event_count: number;
  stats: SessionStats;
  turns: RenderedTurn[];
}

export interface SessionStats {
  cwd: string | null;
  git_branch: string | null;
  duration_secs: number | null;
  models: ModelUsage[];
  tokens: TokenBreakdown;
  context_tokens: number;
}

export interface ModelUsage {
  model: string;
  turn_count: number;
}

export interface TokenBreakdown {
  input: number;
  output: number;
  cache_creation: number;
  cache_read: number;
}

export type RenderedTurn =
  | {
      type: "user";
      timestamp: string | null;
      text: string;
      uuid: string;
      edit_index: number;
      fork_index: number;
      edit_uuid: string;
      fork_uuid: string;
      editable: boolean;
    }
  | {
      type: "assistant";
      timestamp: string | null;
      model: string | null;
      blocks: RenderedBlock[];
      uuid: string;
      edit_index: number;
      fork_index: number;
      edit_uuid: string;
      fork_uuid: string;
      editable: boolean;
    }
  | { type: "system"; timestamp: string | null; content: string }
  | { type: "summary"; timestamp: string | null; content: string }
  | { type: "compaction"; messages_before: number };

export type RenderedBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string }
  | {
      type: "tool_call";
      id: string;
      name: string;
      input_summary: string;
      input_full: Record<string, unknown>;
      result: ToolCallResult | null;
    };

export interface ToolCallResult {
  text: string;
  is_error: boolean;
}

export interface EditableTurn {
  index: number;
  uuid: string;
  role: string;
  text: string;
}

export interface EditableSession {
  source_path: string;
  cwd: string;
  git_branch: string | null;
  turns: EditableTurn[];
}

export interface Violation {
  kind: string;
  detail: string;
}

export interface CommitResult {
  new_session_id: string;
  new_path: string;
  repairs: Violation[];
}
