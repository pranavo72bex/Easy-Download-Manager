# How the window looks and reads

General interface rules, for the parts of a design EDM's own documents leave open. EDM's
documents win: `docs/behaviour-contract.md`, then `docs/ui-design.md`, then
`.claude/skills/edm-design/SKILL.md` ([desktop.md](desktop.md)).

`vendor/ui-ux-pro-max/` holds a checklist taken verbatim from an MIT-licensed skill
([README.md](README.md)): `quick-reference.md` (the rules by category, most important first),
`app-interface.csv` (rules for native apps on phones) and `ux-guidelines.csv` (rules with a
Platform column). Search them with `grep_search` for the words of your task.

## Which of them hold for EDM

EDM is a native window drawn with GPUI, not a web page. Of the checklist:

- **Hold:** colour contrast (4.5:1 for text, 3:1 for large text and marks); a visible focus
  and a keyboard path to everything; an accessible name for every icon-only control; never
  colour alone (add a mark or a word); text that grows with the system's text size without
  being cut; reduced motion followed; a way back or out of every dialog and step; touch
  targets of at least 48 dp on the phone, 8 dp apart; press feedback; no hover-only action;
  an error said next to where it happened, with what to do; a button that starts something
  slow shows that it is working.
- **Do not hold:** anything about CSS, HTML, the DOM, ARIA attributes, Tailwind, React,
  web fonts, images and their formats, SEO, page loading and Core Web Vitals. Rows whose
  Platform is `Web` do not apply. EDM ships no font (the system's is used).
- **Where they disagree with EDM's docs**, follow EDM's docs.

## Lessons

