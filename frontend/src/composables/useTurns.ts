// Fold the flat, seq-ordered stream-json event list into turns.
//
// The CLI runs with --replay-user-messages, so every turn's input is echoed
// back on stdout as a plain-text `type:"user"` event, and the turn ends with the
// existing `type:"result"` event. Turn boundaries are therefore fully derivable
// from markers already in the persisted stream — no turn table, no schema change.
//
//   user(text) → opens a turn (the operator/trigger input)
//   …assistant / tool_use / tool_result events…
//   result     → closes the turn (the output summary)
//
// A `type:"user"` event that carries tool_result blocks (not plain text) is the
// tool-result carrier, NOT a turn opener — it stays inside the current turn.
// Events before the first user event (the system:init) seed an implicit turn.

import { stringifyToolBody } from "./useClaudeStream";

/** Parsed `type:"result"` summary — the turn's output at a glance. */
export interface ClaudeResultSummary {
  isError: boolean;
  resultText: string;
  costUsd?: number;
  numTurns?: number;
  inputTokens?: number;
  outputTokens?: number;
}

export interface Turn {
  index: number;
  inputText: string; // from the opening plain-text user event ("" for the implicit init turn)
  events: unknown[]; // every raw event of this turn, natural order (for the expanded view)
  result?: ClaudeResultSummary; // parsed from the closing result event; absent while streaming
  open: boolean; // still streaming — no result yet
}

/** The plain-text input of a turn-opening `user` event, or null if `ev` isn't one. */
function userInputText(ev: unknown): string | null {
  const e = ev as { type?: string; message?: { content?: unknown } } | null;
  if (!e || e.type !== "user") return null;
  const content = e.message?.content;
  if (typeof content === "string") return content.trim() ? content : null;
  if (Array.isArray(content)) {
    // tool_result carrier — part of the running turn, not a new input.
    if (content.some((c) => (c as { type?: string })?.type === "tool_result")) return null;
    const text = stringifyToolBody(content).trim();
    return text.length ? text : null;
  }
  return null;
}

function parseResult(ev: unknown): ClaudeResultSummary {
  const e = ev as Record<string, any>;
  const usage = e.usage ?? {};
  return {
    isError: !!e.is_error,
    resultText: typeof e.result === "string" ? e.result : "",
    costUsd: typeof e.total_cost_usd === "number" ? e.total_cost_usd : undefined,
    numTurns: typeof e.num_turns === "number" ? e.num_turns : undefined,
    inputTokens: typeof usage.input_tokens === "number" ? usage.input_tokens : undefined,
    outputTokens: typeof usage.output_tokens === "number" ? usage.output_tokens : undefined,
  };
}

/** Group a seq-ordered event array into turns by the user/result markers. */
export function groupIntoTurns(events: unknown[]): Turn[] {
  const turns: Turn[] = [];
  let current: Turn | null = null;

  for (const ev of events) {
    const input = userInputText(ev);
    if (input !== null) {
      // claude replays user messages, so an identical opener can land twice
      // back-to-back. Collapse the echo: same text, previous turn still open and
      // carrying only user events (no work done yet) → fold in, don't re-open.
      const prev = turns[turns.length - 1];
      if (
        prev &&
        prev.open &&
        prev.inputText === input &&
        prev.events.every((e) => (e as { type?: string })?.type === "user")
      ) {
        prev.events.push(ev);
        continue;
      }
      current = { index: turns.length, inputText: input, events: [ev], open: true };
      turns.push(current);
      continue;
    }

    if (!current) {
      // Events before the first input (system:init, or an in-flight turn killed
      // before its result) — seed an implicit, input-less turn.
      current = { index: turns.length, inputText: "", events: [], open: true };
      turns.push(current);
    }
    current.events.push(ev);

    if ((ev as { type?: string })?.type === "result") {
      current.result = parseResult(ev);
      current.open = false;
      current = null; // closed — the next event opens a fresh turn
    }
  }

  return turns;
}
