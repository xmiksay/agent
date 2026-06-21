import { describe, it, expect } from "vitest";
import { groupIntoTurns } from "./useTurns";

const userText = (text: string) => ({ type: "user", message: { content: [{ type: "text", text }] } });
const toolResultCarrier = (id: string) => ({
  type: "user",
  message: { content: [{ type: "tool_result", tool_use_id: id, content: "ok" }] },
});
const assistant = (text: string) => ({ type: "assistant", message: { content: [{ type: "text", text }] } });
const result = (over: Record<string, unknown> = {}) => ({
  type: "result",
  is_error: false,
  result: "summary",
  total_cost_usd: 0.02,
  num_turns: 1,
  usage: { input_tokens: 10, output_tokens: 20 },
  ...over,
});

describe("groupIntoTurns", () => {
  it("returns no turns for an empty stream", () => {
    expect(groupIntoTurns([])).toEqual([]);
  });

  it("opens a turn on a user input and closes it on result", () => {
    const turns = groupIntoTurns([userText("fix it"), assistant("ok"), result()]);
    expect(turns).toHaveLength(1);
    expect(turns[0]).toMatchObject({ index: 0, inputText: "fix it", open: false });
    expect(turns[0].events).toHaveLength(3);
    expect(turns[0].result).toMatchObject({
      isError: false,
      resultText: "summary",
      costUsd: 0.02,
      numTurns: 1,
      inputTokens: 10,
      outputTokens: 20,
    });
  });

  it("seeds an implicit input-less turn for events before the first user input", () => {
    const init = { type: "system", subtype: "init", session_id: "s1" };
    const turns = groupIntoTurns([init, assistant("hi"), result()]);
    expect(turns).toHaveLength(1);
    expect(turns[0].inputText).toBe("");
    expect(turns[0].open).toBe(false);
  });

  it("treats a tool_result carrier as part of the running turn, not a new opener", () => {
    const turns = groupIntoTurns([userText("go"), toolResultCarrier("tu-1"), result()]);
    expect(turns).toHaveLength(1);
    expect(turns[0].events).toHaveLength(3);
  });

  it("splits multiple input→result cycles into separate turns", () => {
    const turns = groupIntoTurns([
      userText("first"),
      result(),
      userText("second"),
      result(),
    ]);
    expect(turns.map((t) => t.inputText)).toEqual(["first", "second"]);
    expect(turns.every((t) => !t.open)).toBe(true);
    expect(turns[1].index).toBe(1);
  });

  it("marks a turn still streaming as open when there is no result yet", () => {
    const turns = groupIntoTurns([userText("go"), assistant("working")]);
    expect(turns[0].open).toBe(true);
    expect(turns[0].result).toBeUndefined();
  });

  it("collapses a replayed identical user opener while the turn is still empty", () => {
    const turns = groupIntoTurns([userText("same"), userText("same"), assistant("ok"), result()]);
    expect(turns).toHaveLength(1);
    expect(turns[0].events).toHaveLength(4);
  });

  it("does not collapse a repeated input once the turn has done work", () => {
    const turns = groupIntoTurns([userText("same"), assistant("worked"), userText("same")]);
    expect(turns).toHaveLength(2);
  });

  it("reports an error result", () => {
    const turns = groupIntoTurns([userText("go"), result({ is_error: true, result: "failed" })]);
    expect(turns[0].result).toMatchObject({ isError: true, resultText: "failed" });
  });
});
