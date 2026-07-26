import { describe, it, expect } from "vitest";
import { buildLineageTree } from "./lineageTree";
import type { SessionSummary } from "../types";

const mk = (over: Partial<SessionSummary> & { id: string }): SessionSummary => ({
  file_path: `/p/${over.id}.jsonl`,
  project_label: "proj",
  cwd: null,
  git_branch: null,
  title: over.id,
  mtime: { secs_since_epoch: 0, nanos_since_epoch: 0 },
  last_event_at: null,
  awaiting_input: false,
  total_tokens: 0,
  forked_from: null,
  copied_from: null,
  copied_overlap: null,
  forked_from_message: null,
  copied_from_message: null,
  ...over,
});

describe("buildLineageTree", () => {
  it("roots from a deep child and classifies fork vs copy edges", () => {
    const root = mk({ id: "root" });
    const fork = mk({ id: "fork", forked_from: "root", forked_from_message: "m1" });
    const copy = mk({ id: "copy", copied_from: "root", copied_from_message: "m2", copied_overlap: 5 });
    const grand = mk({ id: "grand", forked_from: "fork", forked_from_message: "m3" });

    const tree = buildLineageTree([grand, copy, fork, root], "grand")!;
    expect(tree.session.id).toBe("root");
    expect(tree.edge).toBe("root");
    const ids = tree.children.map((c) => c.session.id).sort();
    expect(ids).toEqual(["copy", "fork"]);
    const forkNode = tree.children.find((c) => c.session.id === "fork")!;
    expect(forkNode.edge).toBe("forked");
    expect(forkNode.branchMessageUuid).toBe("m1");
    expect(forkNode.branchParentFilePath).toBe("/p/root.jsonl");
    expect(forkNode.children[0].session.id).toBe("grand");
    const copyNode = tree.children.find((c) => c.session.id === "copy")!;
    expect(copyNode.edge).toBe("copied");
    expect(copyNode.copiedOverlap).toBe(5);
  });

  it("returns null for an unknown center", () => {
    expect(buildLineageTree([mk({ id: "a" })], "nope")).toBeNull();
  });

  it("does not hang on a cycle", () => {
    const a = mk({ id: "a", forked_from: "b" });
    const b = mk({ id: "b", forked_from: "a" });
    const tree = buildLineageTree([a, b], "a");
    expect(tree).not.toBeNull();
  });
});
