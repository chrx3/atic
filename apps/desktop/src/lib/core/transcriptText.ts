/** Texto que Whisper inventa sobre silencio o estática, no habla real. */

export function isJunkTranscriptText(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return true;
  if (isSilenceMarker(trimmed)) return true;
  return isRepetitionHallucination(trimmed);
}

function isSilenceMarker(text: string): boolean {
  const compact = text.replace(/\s+/g, "").toLowerCase();
  return (
    compact === "[silence]" ||
    compact === "(silence)" ||
    compact === "[silence]." ||
    compact === "(silence)." ||
    compact === "[blank_audio]" ||
    compact === "[blankaudio]" ||
    compact === "[inaudible]" ||
    compact === "(inaudible)" ||
    compact === "[music]" ||
    compact === "(music)" ||
    compact === "silence" ||
    compact.startsWith("[silence") ||
    compact.startsWith("(silence") ||
    compact.startsWith("[music") ||
    compact.startsWith("(music") ||
    compact.startsWith("[blank")
  );
}

function tokenizeWords(text: string): string[] {
  return text
    .toLocaleLowerCase()
    .split(/[^0-9a-záéíóúüñ']+/i)
    .filter(Boolean);
}

function isRepetitionHallucination(text: string): boolean {
    const words = tokenizeWords(text);
    if (words.length < 6) return false;

    let run = 1;
  for (let i = 1; i < words.length; i++) {
    if (words[i] === words[i - 1]) {
      run += 1;
      if (run >= 6) return true;
    } else {
      run = 1;
    }
  }

  const counts = new Map<string, number>();
  for (const word of words) {
    counts.set(word, (counts.get(word) ?? 0) + 1);
  }
  const unique = counts.size;
  const total = words.length;
  if (total >= 12 && unique / total <= 0.28) return true;
  for (const n of counts.values()) {
    if (n >= 8 && n / total >= 0.35) return true;
  }

  for (let n = 2; n <= 4; n++) {
    if (maxConsecutiveNgramRepeats(words, n) >= 4) return true;
  }
  return false;
}

function maxConsecutiveNgramRepeats(words: string[], n: number): number {
  if (n === 0 || words.length < n * 2) return 1;
  let best = 1;
  for (let i = 0; i + n * 2 <= words.length; i++) {
    const first = words.slice(i, i + n);
    let repeats = 1;
    let j = i + n;
    while (j + n <= words.length && sameSlice(words, j, first)) {
      repeats += 1;
      j += n;
    }
    if (repeats > best) best = repeats;
  }
  return best;
}

function sameSlice(words: string[], start: number, ngram: string[]): boolean {
  for (let k = 0; k < ngram.length; k++) {
    if (words[start + k] !== ngram[k]) return false;
  }
  return true;
}
