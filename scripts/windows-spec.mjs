// Evidence carriers are acceptance bookkeeping, not a new behavior/case owner.
// Keep unknown/multiline content: this repository's carriers are single-line.
export function qualificationIntent(source) {
  return source.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, frontmatter => frontmatter.replace(/^(?:status|provisional|last_updated):[^\r\n]*(?:\r?\n|$)/gm, ''))
    .replace(/^\*\*(?:Evidence|CoveredBy|Status):\*\*[^\r\n]*(?:\r?\n|$)/gm, '')
    .replace(/\[Source: [^\]\r\n]+\]/g, '')
    .replace(/\r\n/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
}
