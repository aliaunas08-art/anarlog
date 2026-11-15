import type { AskScope } from "./scope";

export type AskIntent =
  | "meeting_fact"
  | "library_fact"
  | "work_product"
  | "general_needed"
  | "unrelated";

export type AskClassification = {
  intent: AskIntent;
  refersToPrior: boolean;
  mentionsOtherMeeting: boolean;
  /** The user explicitly asked to change meeting content or organize meetings. */
  explicitEdit: boolean;
  /** "rules" when a heuristic decided; "ambiguous" when no rule matched. */
  decidedBy: "rules" | "ambiguous" | "model";
};

export type AskClassifierContext = {
  meetingTitle?: string | null;
  /** Content words from the meeting: title, participants, transcript, notes. */
  meetingTerms?: Iterable<string>;
  /** Titles of meetings other than the current one. */
  otherMeetingTitles?: string[];
  /** Whether the thread already has an assistant turn to refer back to. */
  hasPriorTurns?: boolean;
};

const STOPWORDS = new Set(
  `a an the and or but if then so of to in on at by for with about from into over
  under as is are was were be been being am do does did done doing have has had
  having it its this that these those there here what whats which who whom whose
  when where why how can could should would will shall may might must i me my mine
  we us our ours you your yours he him his she her hers they them their theirs not
  no yes all any some more most less least very just also than too again ever
  please tell give show explain define describe know think get got make made let
  like want need one two out up down off s t don doesn didn isn aren wasn weren
  im ive youre thats whats meeting meetings call calls`
    .split(/\s+/)
    .filter(Boolean),
);

export function normalizeText(text: string): string {
  return text
    .toLowerCase()
    .replace(/[’‘]/g, "'")
    .replace(/[^\p{L}\p{N}'\s-]/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** Lowercased content words (length >= 2, no stopwords). */
export function contentWords(text: string): string[] {
  return normalizeText(text)
    .split(" ")
    .map((word) => word.replace(/^'+|'+$/g, "").replace(/'s$/, ""))
    .filter((word) => word.length >= 2 && !STOPWORDS.has(word));
}

/** Builds the meeting vocabulary used for overlap checks. */
export function extractMeetingTerms(
  texts: Array<string | null | undefined>,
): Set<string> {
  const terms = new Set<string>();
  for (const text of texts) {
    if (!text) continue;
    for (const word of contentWords(text)) {
      terms.add(word);
    }
  }
  return terms;
}

function overlapsVocabulary(
  query: string,
  terms: Set<string>,
  ignore: Set<string> = new Set(),
): string[] {
  return contentWords(query).filter(
    (word) => !ignore.has(word) && terms.has(word) && !/^\d+$/.test(word),
  );
}

// Words that tie a request to a meeting even without vocabulary overlap.
const MEETING_REFERENCE =
  /\b(we|we'd|we're|we've|us|our|they|team|meeting|call|sync|standup|transcript|notes?|minutes|summary|recap|agenda|attendees?|participants?|everyone|anyone|someone|speaker|discuss(ed|ion)?|decid(e|ed|ing)|decisions?|agreed?|said|say|mention(ed)?|talk(ed)?|bring up|brought up|raised|action items?|next steps?|follow[- ]?ups?|todos?|to-dos?|owners?|assigned|commit(ted|ments?)?|blockers?|risks?|timeline|plan(s|ned)?)\b/;

// Producing something from the meeting.
const WORK_PRODUCT =
  /\b(draft|write|compose|prepare|create|summari[sz]e|summary|recap|minutes|action items?|follow[- ]?up|email|e-mail|message|slack|agenda|talking points|outline|bullet(s| points)?|list|rewrite|reword|shorten|condense|expand|revise|translate|key (points|takeaways|decisions))\b/;

// Definitions / explanations of a term.
const DEFINITION =
  /^(?:can you |could you |please )?(?:explain|define|describe|what(?:'s| is| are| does| do)|whats|who(?:'s| is| are)|meaning of|tell me (?:about|what))\b\s*(?:a |an |the )?(.*)$/;

// Clearly off-topic tasks for a meeting assistant.
const OFF_TOPIC =
  /\b(homework|essay|poem|haiku|song lyrics|joke|riddle|recipe|horoscope|weather|sports? scores?|lottery|stock price|crypto|bitcoin|write (?:me )?(?:a |some )?(?:code|program|script|function)|python|javascript|typescript|regex|sql query|leetcode|solve (?:this|the) (?:equation|math)|math problem|capital of|translate this (?:sentence|text)|tell me a story|fun fact)\b/;

const OTHER_MEETING_PHRASE =
  /\b(other|another|previous|prior|last|earlier|next|different|older|yesterday'?s?|last week'?s?|last month'?s?)\s+(meeting|call|session|sync|standup|1:1|one-on-one|interview|review)s?\b|\b(all|my|other|past) meetings\b|\bacross (my |all )?meetings\b|\bin (the|my) (library|other meetings)\b/;

const PRIOR_REFERENCE =
  /\b(the|that|this|your|my) (draft|email|e-mail|message|reply|answer|response|list|version|text|paragraph|bullets?)\b|\b(above|previous|earlier|last) (answer|reply|response|draft|message|version)\b|\b(again|shorter|longer|more formal|less formal|more casual|friendlier|punchier|revise|rewrite|reword|rephrase|tweak|redo|continue|keep going|go on|expand on (it|that)|make it|change it|fix it|edit it|shorten it|turn (it|that) into|add (it|that|this) to)\b/;

const MEETING_ARTIFACT =
  /\b(summary|summaries|notes?|memo|minutes|transcript|title|agenda)\b/;

const EXPLICIT_EDIT =
  /\b(edit|update|change|rewrite|revise|fix|correct|replace|apply|shorten|lengthen|restructure|refocus|rename|move|organi[sz]e|file|create (a )?folder|add .{0,40} to (the|my|this) (notes?|memo|summary|agenda|folder))\b|\bmake (the|my|this) (current )?(summary|notes?|memo) (shorter|longer|more|less)\b|\bit'?s not\b.{1,60}\b(it'?s|but)\b|\bprepare (an? )?(agenda|talking points)\b/;

function hasOtherMeetingTitle(
  query: string,
  otherTitles: string[] | undefined,
  currentTitle: string | null | undefined,
): boolean {
  if (!otherTitles || otherTitles.length === 0) return false;
  const normalizedQuery = ` ${normalizeText(query)} `;
  const current = currentTitle ? normalizeText(currentTitle) : null;
  for (const title of otherTitles) {
    const normalized = normalizeText(title);
    // Short or generic titles ("Meeting", "Untitled") match too much.
    if (
      normalized.length < 5 ||
      normalized === current ||
      contentWords(normalized).length === 0 ||
      /^(untitled|new note|meeting|call|sync|standup)( \d+)?$/.test(normalized)
    ) {
      continue;
    }
    if (current && current.includes(normalized)) continue;
    if (normalizedQuery.includes(` ${normalized} `)) {
      return true;
    }
  }
  return false;
}

export function detectRefersToPrior(
  query: string,
  hasPriorTurns: boolean,
): boolean {
  if (!hasPriorTurns) return false;
  const normalized = normalizeText(query);
  if (!normalized) return false;
  const priorCue = PRIOR_REFERENCE.test(normalized);
  const namesPriorObject =
    /\b(draft|email|e-mail|message|reply|answer|response|version)\b/.test(
      normalized,
    );
  // "Make the summary shorter" edits meeting content, not an earlier reply.
  // "Add that to the notes" still carries an earlier reply over.
  if (MEETING_ARTIFACT.test(normalized) && !namesPriorObject) {
    return /\b(add|put|save|insert|use) (it|that|this|those)\b/.test(
      normalized,
    );
  }
  if (priorCue) return true;
  // Short follow-ups that start with a pronoun ("that's too long", "it needs a CTA").
  const words = normalized.split(" ");
  return words.length <= 6 && /^(it|that|this|those|these)\b/.test(normalized);
}

export function detectExplicitEdit(query: string): boolean {
  return EXPLICIT_EDIT.test(normalizeText(query));
}

/**
 * Rule-based classification. Runs before any model call and never needs the
 * transcript in a prompt — only a word set built locally from it.
 */
export function classifyAskQuery(
  query: string,
  scope: AskScope,
  context: AskClassifierContext = {},
): AskClassification {
  const normalized = normalizeText(query);
  const terms =
    context.meetingTerms instanceof Set
      ? (context.meetingTerms as Set<string>)
      : new Set(context.meetingTerms ?? []);
  const refersToPrior = detectRefersToPrior(
    query,
    context.hasPriorTurns ?? false,
  );
  const explicitEdit = detectExplicitEdit(query);
  const mentionsOtherMeeting =
    scope === "meeting" &&
    (OTHER_MEETING_PHRASE.test(normalized) ||
      hasOtherMeetingTitle(
        query,
        context.otherMeetingTitles,
        context.meetingTitle,
      ));

  const result = (
    intent: AskIntent,
    decidedBy: AskClassification["decidedBy"] = "rules",
  ): AskClassification => ({
    intent,
    refersToPrior,
    mentionsOtherMeeting,
    explicitEdit,
    decidedBy,
  });
  const factIntent: AskIntent =
    scope === "library" ? "library_fact" : "meeting_fact";

  if (scope === "general") {
    return result("general_needed");
  }

  // Follow-ups on a prior output stay in the thread's own task.
  if (refersToPrior) {
    return result("work_product");
  }

  const overlap = overlapsVocabulary(query, terms);
  const definition = normalized.match(DEFINITION);
  const meetingReference = MEETING_REFERENCE.test(normalized);

  if (OFF_TOPIC.test(normalized) && overlap.length === 0) {
    return result("unrelated");
  }

  if (definition) {
    const subject = definition[1] ?? "";
    const subjectOverlap = overlapsVocabulary(subject, terms);
    if (subjectOverlap.length > 0) {
      // A term used in the meeting: answer, labelling general knowledge.
      return result(scope === "meeting" ? "general_needed" : factIntent);
    }
    if (meetingReference) {
      return result(factIntent);
    }
    // "What is an orange" — a general definition with no tie to the meeting.
    return result("unrelated");
  }

  if (
    WORK_PRODUCT.test(normalized) &&
    (meetingReference || overlap.length > 0)
  ) {
    return result("work_product");
  }

  if (meetingReference || overlap.length > 0) {
    return result(factIntent);
  }

  // No tie to the meeting either way: the cheap model may decide; without it
  // (or on its failure) the request is answered, never blocked.
  return result(factIntent, "ambiguous");
}

export type AskModelClassifier = (input: {
  query: string;
  scope: AskScope;
  meetingTitle?: string | null;
}) => Promise<AskIntent | null>;

/**
 * Rules first; the cheap-model fallback runs only when rules are ambiguous.
 * Any fallback failure resolves to answering, never to blocking.
 */
export async function classifyAskQueryWithFallback(
  query: string,
  scope: AskScope,
  context: AskClassifierContext,
  modelClassifier?: AskModelClassifier,
): Promise<AskClassification> {
  const ruled = classifyAskQuery(query, scope, context);
  if (ruled.decidedBy !== "ambiguous" || !modelClassifier) {
    return ruled;
  }
  try {
    const intent = await modelClassifier({
      query,
      scope,
      meetingTitle: context.meetingTitle,
    });
    if (!intent) return ruled;
    return { ...ruled, intent, decidedBy: "model" };
  } catch (error) {
    console.warn("[ask-guardrails] classifier fallback failed", error);
    return ruled;
  }
}

export const ASK_CLASSIFIER_PROMPT = `Classify one user request sent to a meeting assistant. No meeting content is provided.
Reply with exactly one word:
- meeting_fact: asks about something said or decided in a meeting
- library_fact: asks about something across the user's meetings
- work_product: asks to draft, summarize, list, or explain something based on a meeting
- general_needed: asks to explain a term or name that could appear in a meeting
- unrelated: anything else (general trivia, homework, coding, essays, jokes)`;

export function parseClassifierReply(reply: string): AskIntent | null {
  const match = reply
    .toLowerCase()
    .match(
      /\b(meeting_fact|library_fact|work_product|general_needed|unrelated)\b/,
    );
  return (match?.[1] as AskIntent | undefined) ?? null;
}
