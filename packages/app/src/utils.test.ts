import { describe, expect, test } from "vitest";
import { envSubstitution } from "./utils.js";

describe("envSubstitution", () => {
  test("simple", () => {
    process.env["test"] = "testValue";
    process.env["otherValue"] = "otherTestValue";
    delete process.env["shouldNotExist"];
    const result = envSubstitution(`
test: \${env:test}
otherValue: "\${env:otherValue}"
shouldNotExist: "\${env:shouldNotExist}"
        `);
    expect(result).toBe(`
test: testValue
otherValue: "otherTestValue"
shouldNotExist: ""
        `);
  });
});
