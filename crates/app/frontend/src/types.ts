export interface SessionSummary {
  id: string;
  file_path: string;
  project_label: string;
  cwd: string | null;
  git_branch: string | null;
  title: string;
  mtime: { secs_since_epoch: number; nanos_since_epoch: number };
  /// Timestamp of the last real conversation turn; null if the session has no
  /// turns yet. Drives the sidebar running/unread indicators.
  last_event_at: { secs_since_epoch: number; nanos_since_epoch: number } | null;
  /// Whether the last turn is a completed assistant turn (claude is done). False
  /// while mid-turn (working/thinking). Separates "running" from "unread".
  awaiting_input: boolean;
  total_tokens: number;
  /// The source session this one was forked from (null if not derived).
  forked_from: string | null;
  /// Ancestor session this one's history was copied from (shared-uuid lineage).
  copied_from: string | null;
  /// Number of messages shared with `copied_from`.
  copied_overlap: number | null;
  /// The parent message uuid this session forked at (explicit fork branch point).
  forked_from_message: string | null;
  /// The parent message uuid a detected copy branched at (its shared-root head).
  copied_from_message: string | null;
}

/// A child session that branched from a given message of the open session.
export interface ForkRef {
  session: SessionSummary;
  kind: "forked" | "copied";
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
      /** Attached/pasted images (absent when none — serde skips empty). */
      images?: RenderedImage[];
      uuid: string;
      edit_index: number;
      fork_index: number;
      edit_uuid: string;
      fork_uuid: string;
      /** Every event uuid this turn covers (one for a user turn). */
      uuids: string[];
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
      /** Every merged assistant event's uuid (head + all merged-in events). */
      uuids: string[];
      editable: boolean;
    }
  | { type: "system"; timestamp: string | null; content: string }
  | { type: "command"; timestamp: string | null; command: string }
  | {
      type: "bash";
      timestamp: string | null;
      /** The `!` command; absent when only its output survived. */
      command: string | null;
      stdout: string;
      stderr: string;
    }
  | { type: "summary"; timestamp: string | null; content: string }
  | {
      type: "task_notification";
      timestamp: string | null;
      uuid: string;
      status: string;
      summary: string;
      result: string;
    }
  | {
      type: "api_error";
      timestamp: string | null;
      /** Claude Code's failure text, e.g. "Prompt is too long". */
      text: string;
    }
  | { type: "compaction"; messages_before: number };

export type RenderedBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string }
  /** Claude Code's "summarized" line: a short summary of the reasoning. */
  | { type: "narration"; text: string }
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
  /** Images the tool returned (absent when none — serde skips empty). */
  images?: RenderedImage[];
}

/** A base64 image ready for a data: URI. */
export interface RenderedImage {
  media_type: string;
  data: string;
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
