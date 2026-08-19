/** Catálogo STT de Groq (mismo ids que `atic_transcribe::GROQ_WHISPER_MODELS`). */

export const GROQ_WHISPER_MODELS = [
  { value: "whisper-large-v3-turbo", label: "Whisper Large v3 Turbo (rápido)" },
  { value: "whisper-large-v3", label: "Whisper Large v3 (más preciso)" },
] as const;

export function groqWhisperLabel(id: string): string {
  const found = GROQ_WHISPER_MODELS.find((model) => model.value === id);
  return found?.label ?? "Groq Whisper";
}
