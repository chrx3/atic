/** Heurística alineada con `atic_audio::looks_like_headset`. */
export function looksLikeHeadset(name: string): boolean {
  const n = name.toLowerCase();
  if (n.includes("stereo mix") || n.includes("mezcla estéreo")) return false;
  return [
    "headset",
    "headphone",
    "auricular",
    "hands-free",
    "handsfree",
    "hands free",
    "hfp",
    "ag audio",
    "communications",
    "comunicación",
    "comunicacion",
    "earphone",
    "earbuds",
    "airpods",
    "wh-",
    "bluetooth",
    "bt ",
    "usb audio",
    "usb headset",
  ].some((k) => n.includes(k));
}
