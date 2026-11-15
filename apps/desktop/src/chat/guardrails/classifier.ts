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
  /** Library scope: number of search hits for the query (local index). */
  libraryHits?: number;
  /** Explicit intent set by a UI action (e.g. the summary chips). */
  explicitIntent?: AskExplicitIntent;
};

export type AskExplicitIntent = "edit_summary";

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
// Pronouns ("we", "our") count for fact questions ("what did we decide")…
const MEETING_REFERENCE =
  /\b(we|we'd|we're|we've|us|our|team|meeting|call|sync|standup|transcript|notes?|minutes|summary|recap|agenda|attendees?|participants?|everyone|anyone|anybody|someone|speaker|discuss(ed|ion)?|decid(e|ed|ing)|decisions?|agreed?|said|say|mention(ed)?|talk(ed)?|bring up|brought up|raised|action items?|next steps?|follow[- ]?ups?|todos?|to-dos?|owners?|assigned|commit(ted|ments?)?|blockers?|risks?|timeline|plan(s|ned)?|conversation)\b/;

// …but only explicit meeting words anchor a definition or an off-topic task
// ("explain quantum physics, we need it" is not about the meeting).
const MEETING_ANCHOR =
  /\b(meeting|call|transcript|notes?|minutes|summary|agenda|attendees?|participants?|discuss(ed|ion)?|decid(ed|ing)|decisions?|agreed|said|mention(ed)?|brought up|raised|action items?|next steps?|conversation)\b/;

// Questions about this meeting that need no vocabulary overlap.
const MEETING_QUESTION =
  /\b(who (was|were) (there|present|in (it|the meeting|the call))|who (attended|joined|showed up|spoke|talked|presented|led)|(spoke|talked) the most|key (points|takeaways|decisions)|main (points|takeaways|topics)|takeaways|highlights|gist|overview|tl ?;? ?dr|summari[sz]e|recap|sum up|action items?|next steps?|questions (i|we) could ask)\b/;

// Producing something from the meeting.
const WORK_PRODUCT =
  /\b(draft|write|compose|prepare|create|summari[sz]e|summary|recap|minutes|action items?|follow[- ]?up|email|e-mail|message|slack|agenda|talking points|outline|bullet(s| points)?|list|rewrite|reword|shorten|condense|expand|revise|translate|key (points|takeaways|decisions)|suggest)\b/;

// Definitions / explanations: captures the determiner and the subject.
const DEFINITION =
  /^(?:can you |could you |please )?(?:explain|define|describe|what(?:'s| is| are| was| were| does| do)|whats|who(?:'s| is| are| was)|meaning of|tell me (?:about|what))\b\s*(a |an |the |our |my |this |that |these |those )?(.*)$/;

// "it", "that", "this about", "this meeting about": the meeting itself.
const DEICTIC_SUBJECT =
  /^(it|that|this|these|those|this about|that about|it about|this all about|this meeting|this call|this meeting about|this call about|going on|happening|the meeting|the call)?\s*(about|all about|for)?\s*$/;

// Clearly off-topic tasks for a meeting assistant. These redirect by rule.
const OFF_TOPIC =
  /\b(homework|essay|poem|haiku|song lyrics|jokes?|riddle|recipes?|cook(ing)?|bake|baking|horoscope|weather|sports? scores?|lottery|stock price|crypto|bitcoin|cover letter|resume|cv|write (?:me )?(?:a |an |some )?(?:code|program|script|function|app)|python|javascript|typescript|regex|sql query|leetcode|solve (?:this|the) (?:equation|math)|math problem|capital of|translate this (?:sentence|text)|tell me a story|fun fact|trivia)\b/;

const ARITHMETIC =
  /^(what'?s|what is|calculate|compute)?\s*[\d\s+\-*/x×÷().=^%]+$/;

// Other meetings. "next meeting" is excluded: planning the next meeting is a
// question about this one.
const OTHER_MEETING_PHRASE =
  /\b(other|another|previous|prior|last|earlier|different|older|yesterday'?s?|last week'?s?|last month'?s?)\s+(meeting|call|session|sync|standup|1:1|one-on-one|interview|review)s?\b|\b(all|my|other|past) meetings\b|\bacross (my |all )?meetings\b|\bin (the|my) (library|other meetings)\b/;

// "Did anyone mention the previous call?" asks about this transcript.
const OTHER_MEETING_AS_TOPIC =
  /\b(mention(ed)?|talk(ed)? about|bring up|brought up|discuss(ed)?|refer(red)? to|reference[ds]?)\b.{0,24}\b(other|another|previous|prior|last|earlier|different)\s+(meeting|call|session|sync|standup)/;

// Earlier outputs a follow-up can refer to. Meeting artifacts (summary,
// notes) are edited from the sources, not from an earlier reply.
const PRIOR_OBJECT =
  /\b(the|that|this|your|my|last|previous|above|earlier) (draft|email|e-mail|message|reply|answer|response|list|version|text|paragraph|bullets?|post|intro|subject line)\b/;

// Edit or continuation verbs for a follow-up.
const PRIOR_VERB =
  /\b(make|shorten|lengthen|rewrite|revise|reword|rephrase|edit|change|fix|tweak|redo|continue|expand|condense|translate|add|remove|cut|drop|polish|improve|update|finish|simplify|turn|trim|soften|tighten|format)\b/;

// "make it shorter", "shorten that", "make that more formal".
const PRONOUN_EDIT =
  /\b(make|keep|get) (it|that|this) (shorter|longer|briefer|more \w+|less \w+|friendlier|punchier|clearer|simpler|tighter|warmer)\b|\b(shorten|lengthen|rewrite|revise|reword|rephrase|tweak|polish|simplify|trim|tighten|soften|translate|condense|expand|continue|finish) (it|that|this)\b/;

const EXPLICIT_EDIT =
  /\b(edit|update|change|rewrite|revise|fix|correct|replace|apply|shorten|lengthen|restructure|refocus|rename|move|organi[sz]e|create (a )?folder|add .{0,40} to (the|my|this) (notes?|memo|summary|agenda|folder))\b|\bmake (the|my|this) (current )?(summary|notes?|memo|minutes)\b.{0,40}\b(shorter|longer|more|less|briefer|concise|detailed)\b|\b(shorter|longer|rewrite|edit|update|fix|change|condense|expand|tighten)\b.{0,40}\b(summary|notes|memo|minutes)\b|\bit'?s not\b.{1,60}\b(it'?s|but)\b|\bprepare (an? )?(agenda|talking points)\b/;

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

/**
 * A follow-up refers to an earlier output only when it names one ("the
 * draft", "your answer") AND asks to edit or continue it, or is a pronoun edit
 * command ("make it shorter"). Bare "again" or "make it" never qualifies.
 */
export function detectRefersToPrior(
  query: string,
  hasPriorTurns: boolean,
): boolean {
  if (!hasPriorTurns) return false;
  const normalized = normalizeText(query);
  if (!normalized) return false;
  if (PRONOUN_EDIT.test(normalized)) return true;
  return PRIOR_OBJECT.test(normalized) && PRIOR_VERB.test(normalized);
}

export function detectExplicitEdit(query: string): boolean {
  return EXPLICIT_EDIT.test(normalizeText(query));
}

function isOffTopic(normalized: string): boolean {
  if (ARITHMETIC.test(normalized.replace(/'s\b/, "'s"))) {
    return /\d/.test(normalized);
  }
  return OFF_TOPIC.test(normalized) && !MEETING_ANCHOR.test(normalized);
}

/**
 * Rule-based classification. Runs before any model call and never needs the
 * transcript in a prompt — only a word set built locally from it.
 *
 * Order: clear off-topic → definitions → follow-ups → meeting questions.
 * In meeting scope, "what/who/when is the X" with no overlap is a meeting
 * question (the model answers "not discussed"); only clear off-topic tasks
 * and definitions of things absent from the meeting are unrelated. In library
 * scope a search hit always makes the request relevant, except for clear
 * off-topic tasks.
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
  const explicitEdit =
    context.explicitIntent === "edit_summary" || detectExplicitEdit(query);
  const mentionsOtherMeeting =
    scope === "meeting" &&
    ((OTHER_MEETING_PHRASE.test(normalized) &&
      !OTHER_MEETING_AS_TOPIC.test(normalized)) ||
      hasOtherMeetingTitle(
        query,
        context.otherMeetingTitles,
        context.meetingTitle,
      ));
  const libraryHits = context.libraryHits ?? 0;

  let refersToPrior = false;
  const result = (
    intent: AskIntent,
    decidedBy: AskClassification["decidedBy"] = "rules",
  ): AskClassification => ({
    intent,
    refersToPrior: intent === "unrelated" ? false : refersToPrior,
    mentionsOtherMeeting,
    explicitEdit,
    decidedBy,
  });
  const factIntent: AskIntent =
    scope === "library" ? "library_fact" : "meeting_fact";

  if (scope === "general") {
    refersToPrior = detectRefersToPrior(query, context.hasPriorTurns ?? false);
    return result("general_needed");
  }

  if (context.explicitIntent === "edit_summary") {
    return result("work_product");
  }

  // 1. Clear off-topic tasks redirect first, whatever else the query says.
  if (isOffTopic(normalized)) {
    return result("unrelated");
  }

  const overlap = overlapsVocabulary(query, terms);
  const definition = normalized.match(DEFINITION);

  // 2. Definitions.
  if (definition) {
    const determiner = (definition[1] ?? "").trim();
    const subject = (definition[2] ?? "").trim();
    if (
      DEICTIC_SUBJECT.test(subject) &&
      !(determiner === "a" || determiner === "an")
    ) {
      // "What's this about?", "explain that", "what is it".
      return result(factIntent);
    }
    const subjectOverlap = overlapsVocabulary(subject, terms);
    if (subjectOverlap.length > 0) {
      // A term used in the meeting: answer, labelling general knowledge.
      return result(scope === "meeting" ? "general_needed" : factIntent);
    }
    if (MEETING_ANCHOR.test(subject) || MEETING_QUESTION.test(normalized)) {
      return result(factIntent);
    }
    if (scope === "library") {
      if (libraryHits > 0) return result(factIntent);
      // "What is an orange": a common-noun definition with no hit.
      if (determiner === "a" || determiner === "an") return result("unrelated");
      return result(factIntent, "ambiguous");
    }
    // Meeting scope: "what is the deadline" / "who is the owner" / "who is
    // Dana" are meeting questions the model answers or says "not discussed".
    if (
      (determiner && determiner !== "a" && determiner !== "an") ||
      /^who/.test(normalized)
    ) {
      return result(factIntent);
    }
    // "What is an orange", "explain quantum physics": absent from the meeting.
    return result("unrelated");
  }

  // 3. Follow-ups on an earlier output of this thread.
  refersToPrior = detectRefersToPrior(query, context.hasPriorTurns ?? false);
  if (refersToPrior) {
    return result("work_product");
  }

  const meetingReference = MEETING_REFERENCE.test(normalized);
  const meetingQuestion = MEETING_QUESTION.test(normalized);

  if (
    WORK_PRODUCT.test(normalized) &&
    (meetingReference || meetingQuestion || overlap.length > 0)
  ) {
    return result("work_product");
  }
  if (meetingQuestion) {
    return result(
      /summari|recap|sum up|key |main |takeaway|tl/.test(normalized)
        ? "work_product"
        : factIntent,
    );
  }
  if (meetingReference || overlap.length > 0 || libraryHits > 0) {
    return result(factIntent);
  }
  // Deictic short questions ("why?", "and then?") belong to the meeting.
  if (
    scope === "meeting" &&
    /^(what|who|when|where|why|how)\b/.test(normalized)
  ) {
    return result(factIntent);
  }

  // No tie either way: the cheap model may decide; without it (or on its
  // failure) the request is answered, never blocked.
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
  return applyModelFallback(
    classifyAskQuery(query, scope, context),
    query,
    scope,
    context,
    modelClassifier,
  );
}

/** Second stage: only for ambiguous rule results. */
export async function applyModelFallback(
  ruled: AskClassification,
  query: string,
  scope: AskScope,
  context: AskClassifierContext,
  modelClassifier?: AskModelClassifier,
): Promise<AskClassification> {
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
    // A library search hit means the request is relevant.
    if (intent === "unrelated" && (context.libraryHits ?? 0) > 0) return ruled;
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
