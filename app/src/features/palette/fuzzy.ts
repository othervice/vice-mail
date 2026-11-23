/** Lower case without diacritics, so "lösch" finds "Löschen" and "loschen" does too. */
export function foldText(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

const isWordStart = (text: string, index: number) => index === 0 || /[\s\-_/.,:;()“”"'‘’«»]/u.test(text[index - 1] ?? "");

/**
 * Score of one query word against a text: every character of the word must appear in order.
 * Higher is better: hits at the start of words and runs of consecutive characters count more,
 * gaps less. Null when the word does not match at all.
 */
function scoreWord(word: string, text: string): number | null {
  // A plain substring at a word start beats any scattered match.
  const direct = text.indexOf(word);
  if (direct >= 0) {
    return 100 + word.length * 4 + (isWordStart(text, direct) ? 50 : 0) - Math.min(direct, 30);
  }
  let score = 0;
  let from = 0;
  let previous = -2;
  for (const char of word) {
    const index = text.indexOf(char, from);
    if (index < 0) return null;
    if (index === previous + 1) score += 6;
    else if (isWordStart(text, index)) score += 8;
    else score -= Math.min(index - from, 6);
    score += 2;
    previous = index;
    from = index + 1;
  }
  return score;
}

/**
 * Fuzzy match of a query against the texts of an entry (label first, then extra words such as
 * the group or the shortcut). All words of the query must match somewhere; the label counts
 * double. Null: no match.
 */
export function fuzzyScore(query: string, label: string, extra: readonly string[] = []): number | null {
  const words = foldText(query).split(/\s+/u).filter(Boolean);
  if (words.length === 0) return 0;
  const folded = foldText(label);
  const others = extra.map(foldText);
  let total = 0;
  for (const word of words) {
    const inLabel = scoreWord(word, folded);
    const inExtra = others.reduce<number | null>((best, text) => {
      const score = scoreWord(word, text);
      return score === null ? best : Math.max(best ?? score, score);
    }, null);
    if (inLabel === null && inExtra === null) return null;
    total += Math.max(inLabel === null ? -Infinity : inLabel * 2, inExtra ?? -Infinity);
  }
  return total;
}
