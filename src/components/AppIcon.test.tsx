import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppIcon, hasOfficialIcon } from "./AppIcon";

describe("app icons", () => {
  it("uses licensed official icons and never the ArtCraft mark", () => {
    for (const id of ["photocraft", "vectorcraft", "gridcraft", "pdfcraft"]) {
      expect(hasOfficialIcon(id)).toBe(true);
    }
    // No explicit icon licence upstream, or trademark: generic icons only.
    for (const id of ["artcraft", "soundcraft", "wordcraft", "deckcraft", "cadcraft"]) {
      expect(hasOfficialIcon(id)).toBe(false);
    }
    const { container } = render(<AppIcon appId="artcraft" />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("svg")).not.toBeNull();
  });
});
