import { test } from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { ModelPicker } from "../src/model-picker.ts";
function setup() {
  const dom = new JSDOM(
    '<div><input><button></button><div id="models" hidden></div></div><small></small>',
  );
  const document = dom.window.document;
  const input = document.querySelector("input")!;
  const toggle = document.querySelector("button")!;
  const list = document.getElementById("models")!;
  const status = document.querySelector("small")!;
  const picker = new ModelPicker(input, toggle, list, status);
  const type = (value: string) => {
    input.value = value;
    input.dispatchEvent(new dom.window.Event("input"));
  };
  const key = (value: string) =>
    input.dispatchEvent(
      new dom.window.KeyboardEvent("keydown", { key: value, cancelable: true }),
    );
  const options = () =>
    [...list.querySelectorAll("button")].map((b) => b.textContent);
  return { dom, input, toggle, list, status, picker, type, key, options };
}
test("substring filtering preserves typed text until a matching option is clicked", () => {
  const { input, list, picker, type, options } = setup();
  picker.accept(picker.begin("https://example.com"), [
    "Qwen/Qwen3",
    "deepseek-ai/DeepSeek-V3",
    "Pro/deepseek-ai/DeepSeek-R1",
    "Qwen/Qwen3",
    "",
  ]);
  type("DEEPSEEK");
  assert.equal(input.value, "DEEPSEEK");
  assert.deepEqual(options(), [
    "Pro/deepseek-ai/DeepSeek-R1",
    "deepseek-ai/DeepSeek-V3",
  ]);
  list.querySelectorAll("button")[1].click();
  assert.equal(input.value, "deepseek-ai/DeepSeek-V3");
  assert.equal(list.hidden, true);
});
test("blank query shows all; unmatched text remains editable without invented suggestions", () => {
  const { input, list, picker, type, options } = setup();
  picker.accept(picker.begin("https://example.com"), ["b", "a", "a"]);
  type("");
  assert.deepEqual(options(), ["a", "b"]);
  type("custom-model");
  assert.deepEqual(options(), []);
  assert.match(list.textContent!, /没有符合/);
  assert.equal(input.value, "custom-model");
});
test("keyboard selection is explicit and Escape closes without completing text", () => {
  const { input, list, picker, type, key } = setup();
  picker.accept(picker.begin("https://example.com"), [
    "deepseek-r1",
    "deepseek-v3",
  ]);
  type("deep");
  key("ArrowDown");
  assert.equal(input.value, "deep");
  key("Enter");
  assert.equal(input.value, "deepseek-r1");
  assert.equal(list.hidden, true);
  type("deep");
  key("ArrowDown");
  key("Escape");
  assert.equal(input.value, "deep");
  assert.equal(list.hidden, true);
  type("deep");
  key("ArrowUp");
  key("Enter");
  assert.equal(input.value, "deepseek-v3");
});
test("changing provider or credentials rejects obsolete results", () => {
  const { picker, type, options } = setup();
  const old = picker.begin("https://old.example");
  picker.setEndpoint("https://new.example");
  assert.equal(picker.accept(old, ["wrong"]), false);
  const next = picker.begin("https://new.example");
  picker.invalidate();
  assert.equal(picker.accept(next, ["wrong"]), false);
  type("");
  assert.deepEqual(options(), []);
});
test("fetch feedback is visible and arrow button reopens matching menu", () => {
  const { picker, status, input, toggle, list } = setup();
  const ticket = picker.begin("https://example.com");
  picker.fail(ticket, "HTTP 401");
  assert.match(status.textContent!, /401/);
  picker.accept(ticket, ["deepseek-v3"]);
  input.value = "deep";
  toggle.click();
  assert.equal(list.hidden, false);
  assert.equal(input.value, "deep");
  toggle.click();
  assert.equal(list.hidden, true);
});
