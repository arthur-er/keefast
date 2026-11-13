import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { StrengthMeter } from "./Strength";

afterEach(cleanup);

describe("StrengthMeter", () => {
  it("fills a segment per step of the score, and says the word and the bits", () => {
    render(<StrengthMeter strength={{ bits: 78, score: "good" }} extra="6 words" />);
    const meter = screen.getByRole("img", { name: "Strength: Good, 78 bits" });
    expect(meter.querySelector(".strength")?.getAttribute("data-score")).toBe("3");
    expect(meter.textContent).toBe("Good78 bits · 6 words");
  });

  it("is empty without a strength", () => {
    render(<StrengthMeter strength={null} />);
    const meter = screen.getByRole("img", { name: "Strength: No strength" });
    expect(meter.querySelector(".strength")?.getAttribute("data-score")).toBe("0");
  });
});
