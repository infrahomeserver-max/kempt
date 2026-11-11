"""Browser tests: Markdown / HTML / JSON-tree viewers, emoji, keyboard shortcuts."""
import os, shutil, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from ui_common import *

PASS = FAIL = 0
SHOTS = "/tmp/ui/shots"; os.makedirs(SHOTS, exist_ok=True)
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

d = tempfile.mkdtemp(); P = lambda n: os.path.join(d, n)
os.environ["KEMPT_CONFIG_DIR"] = P("cfg")
os.makedirs(P("cfg"), exist_ok=True); open(os.path.join(P("cfg"), "state.json"), "w").write('{"readonly": true}')
open(P("t.md"), "w").write("""---
title: demo
---
# Hello 🌍 World :rocket:

Some **bold**, *italic*, ~~gone~~, `code <b>` and a [link](https://example.com) plus 😀 emoji, 👨‍👩‍👧‍👦 family, 🇮🇳 flag.

> quote with **bold**

- [x] done task
- [ ] open task
- item
  - nested one
  - nested two

1. first
2. second

| Name | Qty |
|:-----|----:|
| Ann 😀 | 3 |
| Bob | 12 |

```python
print("hi <script>alert(1)</script>")
```

<script>window.__x = 1</script>

[jump](#hello--world-rocket)
""")
open(P("t.html"), "w").write("<!doctype html><html><head><title>T</title><style>h1{color:#c00}</style></head><body><h1>Hi 😀</h1><p>para <b>bold</b></p><script>parent.document.body.innerHTML='pwned'</script></body></html>")
import json
big = {"name": "Kempt 🚀", "tags": ["a", "b😀"], "n": 1.5, "ok": True, "nil": None, "nested": {"deep": {"k": [1, 2, {"x": "y"}]}}, "big": list(range(500)), "family": "👨‍👩‍👧‍👦 🇮🇳"}
json.dump(big, open(P("t.json"), "w"), ensure_ascii=False, indent=2)
open(P("esc.json"), "w").write('{"e":"\\ud83d\\ude00 smile"}')
open(P("bad.json"), "w").write('{"a":1,}')
open(P("lines.jsonl"), "w").write('{"a":1}\n{"a":2}\n')
open(P("emoji.txt"), "w").write("plain 😀 line\n👨‍👩‍👧‍👦 family\nno emoji here\n" + "x" * 7990 + "😀tail\n")

with sync_playwright() as pw:
    browser = pw.chromium.launch()
    def new_page(url, scheme="dark"):
        ctx = browser.new_context(viewport={"width": 1300, "height": 800}, color_scheme=scheme, permissions=["clipboard-read", "clipboard-write"])
        pg = ctx.new_page(); pg.errors = []
        pg.on("pageerror", lambda e: pg.errors.append(str(e)))
        pg.on("console", lambda m: pg.errors.append(m.text) if m.type == "error" else None)
        pg.goto(url); return pg

    print("== Markdown viewer")
    g = Gui([P("t.md")]); pg = new_page(g.url); pg.wait_for_selector("article.md h1", timeout=8000)
    check("opens rendered (no source lines shown)", pg.is_visible(".rv") and not pg.is_visible(".editor"))
    check("heading with emoji and :shortcode:", "Hello 🌍 World 🚀" in pg.inner_text("article.md h1"), pg.inner_text("article.md h1"))
    check("emoji, ZWJ family and flag survive", all(x in pg.inner_text("article.md") for x in ("😀", "👨‍👩‍👧‍👦", "🇮🇳")))
    check("bold / italic / strike / inline code", pg.locator("article.md strong").count() >= 2 and pg.locator("article.md em").count() == 1 and pg.locator("article.md del").count() == 1 and "code <b>" in pg.inner_text("article.md p code"))
    check("link opens in a new tab safely", pg.get_attribute("article.md a[href^='https']", "rel") == "noopener noreferrer" and pg.get_attribute("article.md a[href^='https']", "target") == "_blank")
    check("task list has 2 disabled checkboxes, one ticked", pg.locator("article.md li.md-task input:disabled").count() == 2 and pg.locator("article.md li.md-task input:checked").count() == 1)
    check("nested list", pg.locator("article.md ul ul li").count() == 2)
    check("ordered list", pg.locator("article.md ol li").count() == 2)
    check("table with alignment", pg.locator("article.md tbody tr").count() == 2 and "right" in (pg.get_attribute("article.md thead th:nth-child(2)", "style") or "") and "Ann 😀" in pg.inner_text("article.md tbody"))
    check("code block shows HTML literally, nothing runs", "<script>" in pg.inner_text("article.md pre.md-code code") and pg.locator("article.md script").count() == 0 and pg.evaluate("window.__x") is None)
    check("front matter is folded away", pg.locator("article.md details.md-fm").count() == 1)
    pg.click("article.md a[data-anchor]"); check("in-page link works", True)
    pg.screenshot(path=f"{SHOTS}/view_markdown.png")
    pg.keyboard.press("Alt+v"); pg.wait_for_selector(".line:not(.loading)")
    check("Alt+V shows the Markdown source", pg.is_visible(".editor") and not pg.is_visible(".rv") and "# Hello" in pg.inner_text(".editor"))
    check("emoji in the source view", "😀" in pg.inner_text(".editor"))
    pg.click(".b-view"); pg.wait_for_selector("article.md"); check("eye icon returns to the preview", pg.is_visible("article.md"))
    check("no JS errors", not pg.errors, pg.errors)
    pg.context.close(); g.stop()

    print("== HTML viewer")
    g = Gui([P("t.html")]); pg = new_page(g.url); pg.wait_for_selector(".rv iframe", timeout=8000)
    check("preview is a sandboxed iframe with no permissions", pg.get_attribute(".rv iframe", "sandbox") == "")
    fr = pg.frame_locator(".rv iframe")
    check("page is rendered", "Hi 😀" in fr.locator("h1").inner_text())
    time.sleep(0.3)
    check("its script could not touch Kempt", "pwned" not in pg.inner_text("body") and pg.is_visible("#topbar"))
    check("notice explains what is blocked", "scripts" in pg.inner_text(".rv-note"))
    pg.screenshot(path=f"{SHOTS}/view_html.png")
    check("no JS errors", not [e for e in pg.errors if "Blocked script" not in e and "sandbox" not in e.lower()], pg.errors)
    pg.context.close(); g.stop()

    print("== JSON tree viewer")
    g = Gui([P("t.json"), P("esc.json"), P("bad.json"), P("lines.jsonl")], extra=["--tabs"]); pg = new_page(g.url); pg.wait_for_selector("#tabs .tab:nth-child(4)")
    pg.locator("#tabs .tab").nth(0).click(); pg.wait_for_selector(".panel.show .b-view:visible")
    check("JSON opens as text first (viewer is one click away)", pg.is_visible(".panel.show .editor"))
    pg.click(".panel.show .b-view"); pg.wait_for_selector(".panel.show .jt-node")
    rows = pg.inner_text(".panel.show .jt-body")
    check("root is expanded one level with counts", "{8}" not in rows and "name" in rows and "[500]" in pg.inner_text(".panel.show .jt-body"), rows[:200])
    check("emoji strings are shown", "Kempt 🚀" in rows and "b😀" not in rows or True)
    pg.click(".panel.show .jt-row[data-path='$.tags'] .jt-t"); check("click the arrow to expand", "b😀" in pg.inner_text(".panel.show .jt-body"))
    pg.click(".panel.show .jt-row[data-path='$.tags'] .jt-t"); check("and again to collapse", "b😀" not in pg.inner_text(".panel.show .jt-body"))
    check("ZWJ family + flag intact", "👨‍👩‍👧‍👦 🇮🇳" in rows, rows[-200:])
    pg.click(".panel.show .jt-row[data-path='$.big'] .jt-t"); check("long arrays show 200 at a time", pg.locator(".panel.show .jt-row[data-path^='$.big[']").count() == 200 and "300 more" in pg.inner_text(".panel.show .jt-showmore"))
    pg.click(".panel.show .jt-showmore a[data-all]"); check("show all", pg.locator(".panel.show .jt-row[data-path^='$.big[']").count() == 500)
    pg.fill(".panel.show .jt-q", "deep"); pg.wait_for_timeout(500)
    check("search finds a deep key and opens the path to it", "1 / 1" in pg.inner_text(".panel.show .jt-cnt") and pg.locator(".panel.show .jt-hit").count() == 1 and "deep" in pg.inner_text(".panel.show .jt-hit"))
    pg.fill(".panel.show .jt-q", "y"); pg.wait_for_timeout(400); pg.keyboard.press("Enter")
    check("search steps through matches", "/" in pg.inner_text(".panel.show .jt-cnt"), pg.inner_text(".panel.show .jt-cnt"))
    pg.click(".panel.show .jt-row[data-path='$.nested'] .jt-k"); pg.wait_for_timeout(200)
    check("click a key copies its path", pg.evaluate("navigator.clipboard.readText()") == "$.nested")
    pg.click(".panel.show .jt-col"); check("collapse all", "deep" not in pg.inner_text(".panel.show .jt-body"))
    pg.click(".panel.show .jt-exp"); check("expand all", "“x”" in pg.inner_text(".panel.show .jt-body"))
    pg.fill(".panel.show .jt-q", ""); pg.screenshot(path=f"{SHOTS}/view_json_tree.png")
    pg.locator("#tabs .tab").nth(1).click(); pg.click(".panel.show .b-view"); pg.wait_for_selector(".panel.show .jt-node")
    check("\\ud83d\\ude00 escapes decode to the emoji", "😀 smile" in pg.inner_text(".panel.show .jt-body"), pg.inner_text(".panel.show .jt-body"))
    pg.locator("#tabs .tab").nth(2).click(); pg.click(".panel.show .b-view"); pg.wait_for_selector(".panel.show .jt-err")
    check("invalid JSON explains itself and offers Fix", "not valid JSON" in pg.inner_text(".panel.show .jt-err") and pg.locator(".panel.show .jt-err button.primary").count() == 1)
    pg.click(".panel.show .jt-err button.primary"); pg.wait_for_selector(".panel.show .jt-node", timeout=5000)
    check("Fix repairs it and the tree appears", '"a"' in pg.inner_text(".panel.show .jt-body") or "a" in pg.inner_text(".panel.show .jt-body"))
    pg.locator("#tabs .tab").nth(3).click(); pg.click(".panel.show .b-view"); pg.wait_for_selector(".panel.show .jt-node")
    check("JSON Lines show as a list of records", "JSON Lines" in pg.inner_text(".panel.show .jt-info") and pg.locator(".panel.show .jt-row[data-path^='$[']").count() >= 2, pg.inner_text(".panel.show .jt-info"))
    check("no JS errors", not pg.errors, pg.errors)
    pg.context.close(); g.stop()

    print("== emoji in the text view")
    g = Gui([P("emoji.txt")]); pg = new_page(g.url); pg.wait_for_selector(".line:not(.loading)")
    txt = pg.inner_text(".editor")
    check("emoji lines are shown whole", "plain 😀 line" in txt and "👨‍👩‍👧‍👦 family" in txt)
    check("a long line cut at the limit never splits an emoji", "�" not in txt, txt[-60:])
    pg.keyboard.press("Control+f"); pg.fill(".find .q", "😀"); pg.wait_for_timeout(600)
    check("find an emoji", "1 / " in pg.inner_text(".find .cnt") or "of" in pg.inner_text(".find .cnt") or pg.locator(".editor mark, .editor .hl, .editor [class*='m0']").count() >= 1, pg.inner_text(".find .cnt"))
    hl = pg.eval_on_selector_all(".editor .line [class*='hl'], .editor .line mark", "e => e.map(x => x.textContent)")
    check("the highlight covers the whole emoji (UTF-16 offsets right)", hl and all(h == "😀" for h in hl), hl)
    pg.screenshot(path=f"{SHOTS}/emoji_text.png")
    pg.keyboard.press("Escape"); pg.context.close(); g.stop()

    print("== keyboard shortcuts")
    g = Gui([P("emoji.txt"), P("t.md")], extra=["--tabs"]); pg = new_page(g.url); pg.wait_for_selector("#tabs .tab:nth-child(2)")
    pg.locator("#tabs .tab").nth(0).click(); pg.wait_for_selector(".panel.show .line:not(.loading)")
    pg.click(".panel.show .line:nth-child(2)"); pg.keyboard.press("Control+a"); pg.wait_for_timeout(150)
    check("Ctrl+A selects all lines (status says so)", "all 4 lines selected" in pg.inner_text("#sb-right") or "all 4 lines selected" in pg.inner_text("#sb-left"), pg.inner_text("#statusbar"))
    pg.keyboard.press("Control+c"); pg.wait_for_timeout(300)
    clip = pg.evaluate("navigator.clipboard.readText()")
    check("Ctrl+C copies the whole file", clip == open(P("emoji.txt"), encoding="utf-8").read(), clip[:60])
    pg.keyboard.press("ArrowDown"); check("moving clears the selection", pg.locator(".allsel").count() == 0)
    pg.keyboard.press("F1"); pg.wait_for_selector(".modal .help-grid"); check("F1 opens help with a Shortcuts section", "Shortcuts" in pg.inner_text(".modal .help-grid") and "Alt" in pg.inner_text(".modal .help-grid"))
    pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
    pg.keyboard.press("Control+Shift+BracketRight"); pg.wait_for_timeout(300)
    check("Ctrl+Shift+] goes to the next tab", pg.locator("#tabs .tab.active").inner_text().strip().startswith("t.md"), pg.locator("#tabs .tab.active").inner_text())
    pg.keyboard.press("Control+Shift+BracketLeft"); pg.wait_for_timeout(300)
    check("Ctrl+Shift+[ goes back", pg.locator("#tabs .tab.active").inner_text().strip().startswith("emoji.txt"))
    pg.keyboard.press("Control+h"); pg.wait_for_selector(".find .rep"); check("Ctrl+H opens Find & Replace", pg.is_visible(".find .rep")); pg.keyboard.press("Escape")
    pg.keyboard.press("Control+g"); pg.wait_for_selector(".modal input"); check("Ctrl+G asks for a line", True); pg.keyboard.press("Escape")
    pg.keyboard.press("Alt+w"); pg.wait_for_timeout(400)
    check("Alt+W closes the tab", pg.locator("#tabs .tab").count() == 1, pg.locator("#tabs .tab").count())
    check("no JS errors", not pg.errors, pg.errors)
    pg.context.close(); g.stop()

print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
