import { afterEach, describe, expect, spyOn, test } from "bun:test";
import { resetSourceFailureLog, runText } from "../src/collector";

describe("runText", () => {
  afterEach(() => {
    resetSourceFailureLog();
  });

  test("returns command stdout on success", async () => {
    const output = await runText(["echo", "tinytop"]);
    expect(output.trim()).toBe("tinytop");
  });

  test("returns the fallback and logs once on a non-zero exit", async () => {
    resetSourceFailureLog();
    const errorSpy = spyOn(console, "error").mockImplementation(() => {});
    try {
      const output = await runText(["false"], "FALLBACK");
      expect(output).toBe("FALLBACK");
      expect(errorSpy).toHaveBeenCalledTimes(1);
      expect(String(errorSpy.mock.calls[0][0])).toContain("exited with code");
    } finally {
      errorSpy.mockRestore();
    }
  });

  test("kills a hung command after the timeout and falls back", async () => {
    resetSourceFailureLog();
    const errorSpy = spyOn(console, "error").mockImplementation(() => {});
    try {
      const started = Date.now();
      const output = await runText(["sleep", "10"], "FALLBACK", 40);
      const elapsed = Date.now() - started;

      expect(output).toBe("FALLBACK");
      // The kill path must return promptly, nowhere near the 10s sleep.
      expect(elapsed).toBeLessThan(3000);
      expect(errorSpy).toHaveBeenCalledTimes(1);
      expect(String(errorSpy.mock.calls[0][0])).toContain("timed out");
    } finally {
      errorSpy.mockRestore();
    }
  });

  test("rate-limits repeated failures from the same source", async () => {
    resetSourceFailureLog();
    const errorSpy = spyOn(console, "error").mockImplementation(() => {});
    try {
      await runText(["false"], "FALLBACK");
      await runText(["false"], "FALLBACK");
      await runText(["false"], "FALLBACK");
      // Same source three times within the window: logged exactly once.
      expect(errorSpy).toHaveBeenCalledTimes(1);
    } finally {
      errorSpy.mockRestore();
    }
  });
});
