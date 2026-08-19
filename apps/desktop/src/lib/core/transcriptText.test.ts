import { describe, expect, it } from "vitest";
import { isJunkTranscriptText } from "./transcriptText";

describe("isJunkTranscriptText", () => {
  it("tira marcadores de silencio", () => {
    expect(isJunkTranscriptText("[silence]")).toBe(true);
    expect(isJunkTranscriptText("")).toBe(true);
    expect(isJunkTranscriptText("[Music]")).toBe(true);
  });

  it("tira el bucle típico de Whisper sobre estática", () => {
    const looping =
      "y y y y y los dos de los dos de los dos, y los dos de los dos, " +
      "y los dos de los dos, y los dos de los dos, y los dos de los dos, " +
      "y los dos de los dos, y los dos de los dos. " +
      "¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! " +
      "¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós!";
    expect(isJunkTranscriptText(looping)).toBe(true);
    expect(
      isJunkTranscriptText(
        "¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós! ¡Adiós!",
      ),
    ).toBe(true);
  });

  it("deja habla normal", () => {
    expect(
      isJunkTranscriptText("Hola, cómo estás, bien y vos, todo bien gracias."),
    ).toBe(false);
    expect(isJunkTranscriptText("vale vale, entonces seguimos")).toBe(false);
  });
});
