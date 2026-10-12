type Ticket = { endpoint: string; revision: number };

export class ModelPicker {
  private endpoint = "";
  private revision = 0;
  private models: string[] = [];
  private filtered: string[] = [];
  private active = -1;
  constructor(
    private input: HTMLInputElement,
    private toggle: HTMLElement,
    private list: HTMLElement,
    private status: HTMLElement,
  ) {
    input.addEventListener("input", () => this.open());
    input.addEventListener("focus", () => {
      if (this.models.length) this.open();
    });
    input.addEventListener("keydown", (e) => {
      if (e.isComposing) return;
      if (e.key === "Escape") {
        if (!list.hidden) {
          e.preventDefault();
          e.stopPropagation();
          this.close();
        }
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (list.hidden) this.open();
        if (!this.filtered.length) return;
        this.active =
          this.active < 0
            ? e.key === "ArrowDown"
              ? 0
              : this.filtered.length - 1
            : (this.active +
                (e.key === "ArrowDown" ? 1 : -1) +
                this.filtered.length) %
              this.filtered.length;
        this.highlight();
      } else if (e.key === "Enter" && !list.hidden) {
        e.preventDefault();
        if (this.active >= 0) this.choose(this.filtered[this.active]);
      } else if (e.key === "Tab") this.close();
    });
    toggle.addEventListener("mousedown", (e) => e.preventDefault());
    toggle.addEventListener("click", () => {
      const wasOpen = !list.hidden;
      input.focus();
      if (wasOpen) this.close();
      else this.open();
    });
    list.addEventListener("mousedown", (e) => e.preventDefault());
    input.ownerDocument.addEventListener("pointerdown", (e) => {
      if (!input.parentElement?.contains(e.target as Node)) this.close();
    });
    input.addEventListener("blur", () => {
      if (input.ownerDocument.activeElement !== toggle) this.close();
    });
  }
  setEndpoint(endpoint: string) {
    endpoint = endpoint.trim();
    if (endpoint !== this.endpoint) {
      this.endpoint = endpoint;
      this.invalidate();
    }
  }
  invalidate() {
    this.revision++;
    this.models = [];
    this.list.replaceChildren();
    this.close();
    this.status.textContent = "可手动输入，或获取模型后输入关键词筛选。";
  }
  begin(endpoint: string): Ticket {
    this.setEndpoint(endpoint);
    this.status.textContent = "正在获取模型…";
    return { endpoint: this.endpoint, revision: ++this.revision };
  }
  accept(ticket: Ticket, models: string[]): boolean {
    if (!this.current(ticket)) return false;
    this.models = [
      ...new Set(models.map((m) => m.trim()).filter(Boolean)),
    ].sort();
    this.status.textContent = this.models.length
      ? `已获取 ${this.models.length} 个模型，输入关键词后从下拉菜单选择。`
      : "服务未返回模型，可手动输入模型名称。";
    this.close();
    return true;
  }
  fail(ticket: Ticket, message: string) {
    if (this.current(ticket)) this.status.textContent = `获取失败：${message}`;
  }
  close() {
    this.list.hidden = true;
    this.input.setAttribute("aria-expanded", "false");
    this.input.removeAttribute("aria-activedescendant");
    this.active = -1;
  }
  private current(ticket: Ticket) {
    return (
      ticket.endpoint === this.endpoint && ticket.revision === this.revision
    );
  }
  private open() {
    const query = this.input.value.trim().toLocaleLowerCase();
    this.filtered = this.models.filter((m) =>
      m.toLocaleLowerCase().includes(query),
    );
    this.active = -1;
    this.list.replaceChildren();
    this.input.removeAttribute("aria-activedescendant");
    for (const [index, model] of this.filtered.entries()) {
      const option = this.list.ownerDocument.createElement("button");
      option.type = "button";
      option.tabIndex = -1;
      option.id = `model-option-${index}`;
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", "false");
      option.textContent = model;
      option.addEventListener("click", () => this.choose(model));
      this.list.append(option);
    }
    if (!this.filtered.length) {
      const empty = this.list.ownerDocument.createElement("div");
      empty.className = "model-empty";
      empty.textContent = this.models.length
        ? "没有符合的模型，可继续手动输入"
        : "请先获取模型，或手动输入名称";
      this.list.append(empty);
    }
    this.list.hidden = false;
    this.input.setAttribute("aria-expanded", "true");
  }
  private choose(model: string) {
    this.input.value = model;
    this.close();
    this.input.focus();
    this.close();
    this.input.dispatchEvent(
      new this.input.ownerDocument.defaultView!.Event("change", {
        bubbles: true,
      }),
    );
  }
  private highlight() {
    for (const [index, option] of [...this.list.children].entries())
      option.setAttribute("aria-selected", String(index === this.active));
    const option = this.list.children[this.active] as HTMLElement;
    this.input.setAttribute("aria-activedescendant", option.id);
    option.scrollIntoView?.({ block: "nearest" });
  }
}
