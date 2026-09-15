import { sentences, combineTranslation, type SentencePair } from "./translation.ts";
export function partialTranslation(content: string, pairs: SentencePair[]) {
  const known = new Map(pairs.map(pair => [pair.id, pair]));
  return combineTranslation(content, sentences(content).map(sentence => known.get(sentence.id) ?? { ...sentence, translated: sentence.original }));
}
export async function runTranslation(content: string,
  request: (batch: { id: number; text: string }[]) => Promise<{ id: number; text: string }[]>,
  update: (pairs: SentencePair[], total: number) => void,
  cancelled: () => boolean,
) {
  const source = sentences(content);
  const batches: typeof source[] = [];
  for (let offset = 0; offset < source.length;) {
    const batch = []; let size = 0;
    while (offset < source.length && batch.length < 8 && (size < 2000 || !batch.length)) {
      const sentence = source[offset++]; batch.push(sentence); size += sentence.original.length;
    }
    batches.push(batch);
  }
  let next = 0, stopped = false;
  const translated: SentencePair[] = [];
  update([], source.length);
  async function worker() {
    while (!stopped && !cancelled()) {
      const batch = batches[next++];
      if (!batch) return;
      try {
        const result = await request(batch.map(s => ({ id: s.id, text: s.original })));
        if (stopped || cancelled()) return;
        if (result.length !== batch.length || new Set(result.map(s => s.id)).size !== batch.length) throw new Error("译文编号不完整，请重新翻译");
        const pairs = batch.map(sentence => {
          const item = result.find(s => s.id === sentence.id);
          if (!item) throw new Error("译文编号不匹配，请重新翻译");
          return { ...sentence, translated: item.text };
        });
        const ready = [...translated, ...pairs].sort((a, b) => a.id - b.id);
        partialTranslation(content, ready); // Validate code, links and identities before showing.
        translated.splice(0, translated.length, ...ready);
        update([...translated], source.length);
      } catch (error) { stopped = true; throw error; }
    }
  }
  await Promise.all([worker(), worker()]);
  if (cancelled()) throw new Error("已停止等待");
  return { text: combineTranslation(content, translated), pairs: translated };
}
