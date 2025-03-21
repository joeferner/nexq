import { describe, expect, test } from "vitest";
import { applyConfigOverrides, envSubstitution } from "./utils.js";

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

describe("applyConfigOverrides", () => {
  test("simple", () => {
    const obj = {
      test: "value",
    };
    applyConfigOverrides(obj, ["test=new"]);
    expect(obj).toEqual({
      test: "new",
    });
  });

  test("number", () => {
    const obj = {
      test: "value",
    };
    applyConfigOverrides(obj, ["test=5"]);
    expect(obj).toEqual({
      test: 5,
    });
  });

  test("starts with number", () => {
    const obj = {
      test: "value",
    };
    applyConfigOverrides(obj, ["test=0.0.0.0:7888"]);
    expect(obj).toEqual({
      test: "0.0.0.0:7888",
    });
  });

  test("new value", () => {
    const obj = {
      test: "value",
    };
    applyConfigOverrides(obj, ["newTest=5"]);
    expect(obj).toEqual({
      test: "value",
      newTest: 5,
    });
  });

  test("quotes", () => {
    const obj = {
      test: "value",
    };
    applyConfigOverrides(obj, ['test="5"']);
    expect(obj).toEqual({
      test: "5",
    });
  });

  test("quotes with equals", () => {
    const obj = {
      test: "value",
    };
    applyConfigOverrides(obj, ['test="1=2"']);
    expect(obj).toEqual({
      test: "1=2",
    });
  });

  test("arrays", () => {
    const obj = {
      test: ["a", "b"],
    };
    applyConfigOverrides(obj, ["test.1=c"]);
    expect(obj).toEqual({
      test: ["a", "c"],
    });
  });

  test("nesting", () => {
    const obj = {
      outer: {
        inner: {
          name: "test",
        },
      },
    };
    applyConfigOverrides(obj, ["outer.inner.name=new"]);
    expect(obj).toEqual({
      outer: {
        inner: {
          name: "new",
        },
      },
    });
  });
});
