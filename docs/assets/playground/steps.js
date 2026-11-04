// Steps: a list of playground sessions to walk through, each one built on the one before.
// No DOM, so tests/playground.test.mjs runs it too.

const FIELDS = ["source", "schema", "dialect", "config"];
const PANELS = ["problems", "types", "rewritten", "json", "notes"];
const FILES = ["python", "schema", "config"];

// The steps of a deck as full sessions: a step that leaves a file out keeps the one before.
export function unfold(deck) {
  if (!deck || !Array.isArray(deck.steps) || !deck.steps.length) {
    throw new Error("A deck needs a non-empty `steps` list.");
  }
  let previous = { source: "", schema: "", dialect: "postgres", config: "" };
  return deck.steps.map((step, index) => {
    if (!step || typeof step !== "object") throw new Error(`Step ${index + 1} is not an object.`);
    const full = { ...previous };
    for (const field of FIELDS) {
      if (field in step && typeof step[field] !== "string") {
        throw new Error(`Step ${index + 1}: \`${field}\` is not a string.`);
      }
      if (typeof step[field] === "string") full[field] = step[field];
    }
    previous = { ...full };
    return {
      ...full,
      title: typeof step.title === "string" && step.title ? step.title : `Step ${index + 1}`,
      notes: typeof step.notes === "string" ? step.notes : "",
      panel: PANELS.includes(step.panel) ? step.panel : null,
      file: FILES.includes(step.file) ? step.file : null,
    };
  });
}

// A deck that unfolds to `sessions`, keeping only what changes from one step to the next.
export function fold(sessions, title = "") {
  let previous = {};
  const steps = sessions.map((session) => {
    const step = { title: session.title };
    for (const field of FIELDS) {
      if (session[field] !== previous[field]) step[field] = session[field];
    }
    for (const key of ["notes", "panel", "file"]) if (session[key]) step[key] = session[key];
    previous = session;
    return step;
  });
  return title ? { title, steps } : { steps };
}
