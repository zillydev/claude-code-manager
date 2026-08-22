import { describe, it, expect } from "vitest";
import { askLines, askText, chosenLabels } from "./askQuestion";

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

describe("chosenLabels", () => {
  it("reads the picked option out of the result sentence", () => {
    const r =
      'Your questions have been answered: "Where should the link live?"="Settings". You can now continue with these answers in mind.';
    expect([...chosenLabels(r)]).toEqual(["Settings"]);
  });

  it("splits a multi-select answer", () => {
    const r = 'Your questions have been answered: "Which?"="Project — often, Tags — often".';
    expect([...chosenLabels(r)]).toEqual(["Project — often", "Tags — often"]);
  });

  it("reads several questions in one result", () => {
    const r = '"Q1"="A1" and "Q2"="A2"';
    expect([...chosenLabels(r)]).toEqual(["A1", "A2"]);
  });

  it("marks nothing when the result is an error or missing", () => {
    expect(chosenLabels("<tool_use_error>InputValidationError: …</tool_use_error>").size).toBe(0);
    expect(chosenLabels(null).size).toBe(0);
    expect(chosenLabels("").size).toBe(0);
  });
});
