import { describe, it, expect } from "vitest";
import { askLines, askText, parseAnswers } from "./askQuestion";

const input = {
  questions: [
    {
      question: "Where should the link live?",
      header: "Placement",
      multiSelect: false,
      options: [
        { label: "Settings", description: "In an About section" },
        { label: "Sidebar", description: "Next to the gear" },
      ],
    },
  ],
};

describe("askLines", () => {
  it("renders the header, question and options as lines", () => {
    expect(askText(askLines(input)!)).toBe(
      [
        "Placement · Where should the link live?",
        "  • Settings — In an About section",
        "  • Sidebar — Next to the gear",
      ].join("\n"),
    );
  });

  it("marks a multi-select question and separates several questions", () => {
    const lines = askLines({
      questions: [
        { question: "A?", header: "One", multiSelect: true, options: [{ label: "x" }] },
        { question: "B?", header: "Two", multiSelect: false, options: [{ label: "y" }] },
      ],
    })!;
    expect(askText(lines).split("\n")).toEqual([
      "One · A? (pick any)",
      "  • x",
      "",
      "Two · B?",
      "  • y",
    ]);
  });

  it("keeps each option's label for the renderer to match against", () => {
    const opts = askLines(input)!.filter((l) => l.kind === "option");
    expect(opts.map((l) => l.label)).toEqual(["Settings", "Sidebar"]);
  });

  it("returns null for a shape it doesn't recognise, so JSON is shown instead", () => {
    expect(askLines({})).toBeNull();
    expect(askLines({ questions: [] })).toBeNull();
    expect(askLines({ questions: "nope" })).toBeNull();
    expect(askLines({ questions: [{ header: "h" }] })).toBeNull();
  });
});

const SCREENSHOT = {
  // The reported card: three questions; one answered by picking, two by typing.
  input: {
    questions: [
      { question: "Which option do you want?", header: "Saving green", options: [{ label: "Use status-success-fg (Recommended)" }, { label: "Defer, keep the local token" }] },
      { question: "Should the org-workspace budget check also use the discounted amount?", header: "Org guard", options: [{ label: "Fix it in this PR (Recommended)" }, { label: "Leave it, note as limitation" }] },
      { question: "For the four code-review findings:", header: "Next steps", options: [{ label: "Fix, then post all replies (Recommended)" }, { label: "Skip the code-review items" }] },
    ],
  },
  result:
    'The user answered: "Which option do you want?"="Defer, keep the local token", "Should the org-workspace budget check also use the discounted amount?"="explain me the current behaviour with standard free", "For the four code-review findings:"="no changes". Read the answers carefully.',
};

describe("parseAnswers", () => {
  it("reads a picked option out of the result sentence", () => {
    const r =
      'Your questions have been answered: "Where should the link live?"="Settings". You can now continue with these answers in mind.';
    expect(parseAnswers(r, ["Where should the link live?"]).get("Where should the link live?")).toEqual({ value: "Settings", items: ["Settings"] });
  });

  it("splits a multi-select answer at its commas", () => {
    // Verbatim from a live session: two picks, plain labels.
    const r =
      'Your questions have been answered: "Which fruits do you like?"="Apple, Cherry". You can now continue with these answers in mind.';
    expect(parseAnswers(r, ["Which fruits do you like?"]).get("Which fruits do you like?")!.items).toEqual(["Apple", "Cherry"]);
  });

  it("keeps a label's own commas when Claude Code CSV-quotes it", () => {
    // Verbatim from the reported session: one pick whose label has commas.
    const r =
      "Your questions have been answered: \"Which of the reviewer's six refactors should I do in this PR? I'd reply on every thread either way.\"=\"\"Small ones: 1, 2, 3, 4 (Recommended)\"\". You can now continue with these answers in mind.";
    const q = "Which of the reviewer's six refactors should I do in this PR? I'd reply on every thread either way.";
    expect(parseAnswers(r, [q]).get(q)!.items).toEqual(["Small ones: 1, 2, 3, 4 (Recommended)"]);
  });

  it("splits quoted items at the quote-comma-quote seam only", () => {
    const a = parseAnswers('"Q"=""A, x", "B, y"", "Q2"="C"', ["Q", "Q2"]);
    expect(a.get("Q")!.items).toEqual(["A, x", "B, y"]);
    expect(a.get("Q2")!.items).toEqual(["C"]);
  });

  it("reads a typed answer given with no option picked", () => {
    // Verbatim shape: bare, unquoted, up to the closing sentence.
    const r =
      'Your questions have been answered: "Which key name should the single merged key use?"=(no option selected) notes: include "SMB" in the key name. You can now continue with these answers in mind.';
    expect(parseAnswers(r, ["Which key name should the single merged key use?"]).get("Which key name should the single merged key use?")).toEqual({
      value: 'include "SMB" in the key name',
      items: ['include "SMB" in the key name'],
    });
  });

  it("reads nothing from an error or a missing result", () => {
    expect(parseAnswers("<tool_use_error>InputValidationError: …</tool_use_error>", ["Q"]).size).toBe(0);
    expect(parseAnswers(null, ["Q"]).size).toBe(0);
    expect(parseAnswers("", ["Q"]).size).toBe(0);
  });
});

// Verbatim from a session: the second question itself contains quotes, and
// the closing sentence is the "Read the answers carefully" wording.
const QUOTED_QUESTION = {
  "input": {
    "questions": [
      {
        "question": "Codex P1: should the low-credit check use the lower bar only when the selected deploy actually gets the discount (monthly, Starter tier)?",
        "header": "Guard fix",
        "options": [
          {
            "label": "Fix it in this PR (Recommended)"
          },
          {
            "label": "Leave it, reply to Codex"
          }
        ]
      },
      {
        "question": "Codex P2: where should the replace card's \"Deploy as new app\" price get fixed?",
        "header": "Replace card",
        "options": [
          {
            "label": "Follow-up PR (Recommended)"
          },
          {
            "label": "Discount only, in this PR"
          }
        ]
      }
    ]
  },
  "result": "The user answered: \"Codex P1: should the low-credit check use the lower bar only when the selected deploy actually gets the discount (monthly, Starter tier)?\"=\"maybe it's fixed in staging, fetch latest changes, and accept the narrow gap\", \"Codex P2: where should the replace card's \"Deploy as new app\" price get fixed?\"=\"how does this work for free deployment\". Read the answers carefully — they may request clarification, changes, or that you not proceed — and follow what they actually say."
};

// Verbatim from a session: the picked option carried a preview, which Claude
// Code pastes into the answer sentence after `selected preview:`.
const WITH_PREVIEW = {
  "input": {
    "questions": [
      {
        "question": "What should a file's modified-date be set to after download?",
        "header": "mtime source",
        "options": [
          {
            "label": "Playlist position (synthetic)",
            "description": "Stamp each file's mtime from its playlist index, so sort-by-modified always equals playlist order — regardless of when downloaded or how the playlist is ordered.",
            "preview": "Playlist: [NewA, NewB, Old1, Old2, Old3]\n              idx1  idx2  idx3  idx4  idx5\n\nmtime = base + index x 1min\n\nSort by modified (asc):\n  NewA  (base+1m)\n  NewB  (base+2m)\n  Old1  (base+3m)\n  Old2  (base+4m)\n  Old3  (base+5m)\n\n=> file order ALWAYS matches playlist order"
          },
          {
            "label": "Video upload date",
            "description": "Set mtime to the video's real YouTube publish date (yt-dlp's natural behavior, preserved through post-processing). Matches playlist order only if the playlist is in chronological order.",
            "preview": "mtime = video's upload date\n\nSort by modified (asc) = oldest-uploaded first\nSort by modified (desc) = newest-uploaded first\n\nMatches playlist order ONLY if the\nplaylist happens to be chronological.\nA hand-curated / reordered playlist\nwill NOT match."
          }
        ]
      }
    ]
  },
  "result": "Your questions have been answered: \"What should a file's modified-date be set to after download?\"=\"Playlist position (synthetic)\" selected preview:\nPlaylist: [NewA, NewB, Old1, Old2, Old3]\n              idx1  idx2  idx3  idx4  idx5\n\nmtime = base + index x 1min\n\nSort by modified (asc):\n  NewA  (base+1m)\n  NewB  (base+2m)\n  Old1  (base+3m)\n  Old2  (base+4m)\n  Old3  (base+5m)\n\n=> file order ALWAYS matches playlist order. You can now continue with these answers in mind."
};

describe("askLines with the answers", () => {
  it("marks a pick that came with a preview, and shows that option's preview", () => {
    const lines = askLines(WITH_PREVIEW.input, WITH_PREVIEW.result)!;
    expect(lines.filter((l) => l.chosen).map((l) => l.text)).toEqual([
      "  • Playlist position (synthetic) — " + WITH_PREVIEW.input.questions[0].options[0].description,
    ]);
    const preview = lines.filter((l) => l.kind === "preview").map((l) => l.text.slice(6));
    expect(preview.join("\n")).toBe("Playlist: [NewA, NewB, Old1, Old2, Old3]\n              idx1  idx2  idx3  idx4  idx5\n\nmtime = base + index x 1min\n\nSort by modified (asc):\n  NewA  (base+1m)\n  NewB  (base+2m)\n  Old1  (base+3m)\n  Old2  (base+4m)\n  Old3  (base+5m)\n\n=> file order ALWAYS matches playlist order");
    // Nothing typed, and the other option's preview stays hidden.
    expect(lines.some((l) => l.kind === "typed")).toBe(false);
    expect(lines.some((l) => l.text.includes("upload date\n"))).toBe(false);
  });

  it("reads only the label from a preview-bearing value, even with quotes inside the preview", () => {
    const a = parseAnswers(
      'The user answered: "Q"="Convert me to Standard (Recommended)" selected preview:\n  -> gate returns "was_pro"\n  -> banner never renders. Read the answers carefully — and follow what they say.',
      ["Q"],
    );
    expect(a.get("Q")).toEqual({ value: "Convert me to Standard (Recommended)", items: ["Convert me to Standard (Recommended)"] });
  });

  it("finds a question that contains quotes, by locating it literally", () => {
    const lines = askLines(QUOTED_QUESTION.input, QUOTED_QUESTION.result)!;
    expect(lines.filter((l) => l.kind === "typed").map((l) => l.text)).toEqual([
      "  ↳ maybe it's fixed in staging, fetch latest changes, and accept the narrow gap",
      "  ↳ how does this work for free deployment",
    ]);
    expect(lines.some((l) => l.kind === "option" && l.chosen)).toBe(false);
  });

  it("marks the picked option and gives a typed answer its own row", () => {
    const lines = askLines(SCREENSHOT.input, SCREENSHOT.result)!;
    const marked = lines.filter((l) => l.chosen).map((l) => l.text);
    expect(marked).toEqual([
      "  • Defer, keep the local token",
      "  ↳ explain me the current behaviour with standard free",
      "  ↳ no changes",
    ]);
    // The typed rows sit under their own question, after its options.
    const texts = lines.map((l) => l.text);
    expect(texts.indexOf("  ↳ no changes")).toBe(texts.length - 1);
    expect(texts.indexOf("  ↳ explain me the current behaviour with standard free")).toBeLessThan(
      texts.indexOf("Next steps · For the four code-review findings:"),
    );
  });

  it("never marks a wrong option, and adds no row without an answer", () => {
    const lines = askLines(SCREENSHOT.input)!;
    expect(lines.some((l) => l.chosen)).toBe(false);
    expect(lines.some((l) => l.kind === "typed")).toBe(false);
  });

  it("shows only the leftover text when a pick and typed text are combined", () => {
    const lines = askLines(
      { questions: [{ question: "Fruits?", options: [{ label: "Apple" }, { label: "Banana" }] }] },
      '"Fruits?"="Apple, and a kiwi, please"',
    )!;
    expect(lines.filter((l) => l.chosen).map((l) => l.text)).toEqual(["  • Apple", "  ↳ and a kiwi, please"]);
  });

  it("is the same text the find bar matches against", () => {
    // askText is what both the renderer and the matcher derive; the typed rows
    // must be in it or a hit inside one would paint at the wrong offset.
    expect(askText(askLines(SCREENSHOT.input, SCREENSHOT.result)!)).toContain("  ↳ no changes");
  });
});

// Verbatim from sessions: what Claude Code records when "Chat about this" is picked, and when a question is dismissed.
const CHAT_ABOUT_THIS = `The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:
The user wants to clarify these questions.
    This means they may have additional information, context or questions for you.
    Take their response into account and then reformulate the questions if appropriate.
    Start by asking them what they would like to clarify.

    Questions asked:
- "Where should the link live?"
  (No answer provided)

Note: The user's next message may contain a correction or preference. Pay close attention — if they explain what went wrong or how they'd prefer you to work, consider saving that to memory for future sessions.`;
const DISMISSED = `The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.

Note: The user's next message may contain a correction or preference. Pay close attention — if they explain what went wrong or how they'd prefer you to work, consider saving that to memory for future sessions.`;

describe("askLines when the question wasn't answered", () => {
  it("says the user chose to chat about it, accented like an answer", () => {
    const lines = askLines(input, CHAT_ABOUT_THIS)!;
    expect(lines.filter((l) => l.kind === "option").some((l) => l.chosen)).toBe(false);
    expect(lines[lines.length - 1]).toEqual({ kind: "outcome", chosen: true, text: "  ↳ Chat about this" });
  });

  it("says a dismissed question got no answer, without accent", () => {
    const lines = askLines(input, DISMISSED)!;
    expect(lines[lines.length - 1]).toEqual({ kind: "outcome", chosen: false, text: "  ↳ Dismissed without an answer" });
  });

  it("adds one row after all the questions of a card", () => {
    const two = { questions: [...input.questions, { question: "And the icon?", options: [{ label: "Link" }] }] };
    const lines = askLines(two, CHAT_ABOUT_THIS)!;
    expect(lines.filter((l) => l.kind === "outcome")).toHaveLength(1);
    expect(lines[lines.length - 1].kind).toBe("outcome");
  });

  it("adds nothing for a tool error or a missing result", () => {
    const error = "<tool_use_error>InputValidationError: AskUserQuestion failed due to the following issue:</tool_use_error>";
    expect(askLines(input, error)!.some((l) => l.kind === "outcome")).toBe(false);
    expect(askLines(input)!.some((l) => l.kind === "outcome")).toBe(false);
  });

  it("puts the row in the text the find bar matches", () => {
    expect(askText(askLines(input, CHAT_ABOUT_THIS)!)).toContain("  ↳ Chat about this");
  });
});
