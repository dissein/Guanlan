export function finishStreamMessage(message: HTMLElement): void {
  message.querySelectorAll(".waiting, .stream-dot").forEach((node) => node.remove());
  const body = message.querySelector(".markdown");
  if (body && !body.textContent?.trim()) body.remove();
}
