import { test } from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { finishStreamMessage } from "../src/stream-status.ts";

test("reasoning-only tool rounds leave a single waiting marker in the active round", () => {
  const dom = new JSDOM('<main></main>');
  const document = dom.window.document;
  const conversation = document.querySelector('main')!;
  for (let step = 0; step < 4; step++) {
    const old = conversation.lastElementChild as HTMLElement | null;
    if (old) finishStreamMessage(old);
    const message = document.createElement('article');
    message.innerHTML = '<span class="stream-dot"></span><details>思考过程</details><div class="markdown"><span class="waiting">正在思考…</span></div>';
    conversation.append(message);
    assert.equal(conversation.querySelectorAll('.waiting').length, 1);
    assert.equal(conversation.querySelectorAll('.stream-dot').length, 1);
  }
  finishStreamMessage(conversation.lastElementChild as HTMLElement);
  assert.equal(conversation.querySelectorAll('.waiting,.stream-dot').length, 0);
  assert.equal(conversation.querySelectorAll('details').length, 4);
});

test("finishing a stream preserves its answer and reasoning", () => {
  const dom = new JSDOM('<article><span class="stream-dot"></span><details>思考过程</details><div class="markdown"><p>完成的回答</p></div></article>');
  const message = dom.window.document.querySelector('article')!;
  finishStreamMessage(message);
  finishStreamMessage(message);
  assert.equal(message.querySelector('.markdown')!.textContent, '完成的回答');
  assert.equal(message.querySelector('details')!.textContent, '思考过程');
  assert.equal(message.querySelector('.stream-dot'), null);
});
