---
name: save
description: Create a persistent Markdown file in TCUI Saved files.
---

# Save

Return the complete requested document as Markdown file contents. Reconstruct
referenced content from the conversation when the user says "this" or "that".
Include every section, not a summary or excerpt. Do not add commentary or
surround the document with a code fence. TCUI captures the complete body, saves
it directly as one canonical Saved file, and adds a conversation-scoped Chat
file link. TCUI replaces the user-visible assistant response with
`Saved as <filename>`; do not emit that confirmation instead of the file body.
