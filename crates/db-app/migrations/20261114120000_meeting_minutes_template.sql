-- Seed the built-in "Meeting Minutes" summary template (data only, no DDL).
INSERT OR IGNORE INTO templates (
  id, title, description, pinned, pin_order, category, targets_json, sections_json
) VALUES (
  'default-meeting-minutes',
  'Meeting Minutes',
  'Formal minutes with decisions and an action items table. Rules: merge the user''s typed Meeting Notes with the transcript; when they conflict, the Meeting Notes are correct. Never invent names, owners, dates, or decisions. Use h1 headers with the exact section titles below, in this order, and ignore the minimum bullet count for sections that need fewer.',
  1,
  0,
  'General',
  '["Anyone"]',
  '[{"title":"Meeting Details","description":"Three bullets: Title (the meeting title, or a short descriptive title if none), Date (the meeting date from the context, otherwise the current date), Attendees (names from the participant list, speaker labels, or the user notes; write Not stated if none are known)."},{"title":"Summary","description":"A single paragraph of 3 to 5 sentences, not bullets."},{"title":"Key Decisions","description":"Bullets of decisions that were explicitly agreed. Write None recorded if there were none."},{"title":"Action Items","description":"A markdown table with exactly these columns: Action | Owner | Due date. One row per task. Owner and Due date must be stated in the notes or transcript; otherwise write Not stated. Never guess or infer an owner or a date. If there are no action items, output a single row: None recorded | Not stated | Not stated."},{"title":"Open Questions","description":"Bullets of unresolved questions or topics needing follow-up. Write None recorded if there were none."},{"title":"Transcript","description":"Output exactly this one line and nothing else: The full transcript with speaker labels is kept separately in the Transcript tab."}]'
);
