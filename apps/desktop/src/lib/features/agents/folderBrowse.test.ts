import { describe, expect, it } from "vitest";
import {
  filterEntries,
  foldName,
  isFav,
  jumpIndex,
  leafName,
  pathsEqual,
  toggleFav,
} from "./folderBrowse";

describe("foldName / pathsEqual", () => {
  it("trata tildes como la letra base", () => {
    expect(foldName("Documentos")).toBe("documentos");
    expect(foldName("Área")).toBe("area");
  });

  it("iguala rutas de Windows sin importar barra ni mayúsculas", () => {
    expect(pathsEqual("C:\\Users\\docs\\", "c:/users/docs")).toBe(true);
    expect(pathsEqual("C:\\atic", "D:\\atic")).toBe(false);
  });

  it("saca el último segmento", () => {
    expect(leafName("C:\\Users\\Christian\\atic")).toBe("atic");
    expect(leafName("/home/user/docs/")).toBe("docs");
  });
});

describe("filterEntries", () => {
  const entries = [
    { name: "apps" },
    { name: "docs" },
    { name: "Features" },
    { name: "Área" },
  ];

  it("filtra por subcadena, sin tildes", () => {
    expect(filterEntries(entries, "fe").map((e) => e.name)).toEqual(["Features"]);
    expect(filterEntries(entries, "ar").map((e) => e.name)).toEqual(["Área"]);
  });

  it("sin query deja la lista igual", () => {
    expect(filterEntries(entries, "  ")).toHaveLength(4);
  });
});

describe("jumpIndex", () => {
  const names = ["apps", "crates", "docs", "Features"];

  it("una letra salta a la primera que empieza con ella", () => {
    expect(jumpIndex(names, "d", -1)).toBe(2);
    expect(jumpIndex(names, "F", -1)).toBe(3);
  });

  it("repetir la letra cicla a la siguiente", () => {
    const both = ["docs", "Downloads", "tmp"];
    expect(jumpIndex(both, "d", -1)).toBe(0);
    expect(jumpIndex(both, "d", 0)).toBe(1);
    expect(jumpIndex(both, "dd", 1)).toBe(0);
  });

  it("un prefijo de varias letras no cicla, busca el nombre", () => {
    const both = ["docs", "Downloads"];
    expect(jumpIndex(both, "do", 0)).toBe(0);
    expect(jumpIndex(both, "dow", 0)).toBe(1);
  });

  it("sin coincidencia no mueve", () => {
    expect(jumpIndex(names, "z", 0)).toBe(-1);
  });
});

describe("toggleFav", () => {
  const atic = { name: "atic", path: "C:\\work\\atic" };
  const apps = { name: "apps", path: "C:\\work\\apps" };

  it("añade al frente y quita por ruta", () => {
    const one = toggleFav([], atic);
    expect(one).toEqual([atic]);
    const two = toggleFav(one, apps);
    expect(two[0]).toEqual(apps);
    expect(isFav(two, atic.path)).toBe(true);
    expect(toggleFav(two, { name: "ATIC", path: "c:/work/atic" })).toEqual([
      apps,
    ]);
  });
});
