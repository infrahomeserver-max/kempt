"""Firefox (via geckodriver + Selenium): the same user-facing flows as the Chromium suites, smaller.
Run:  FIREFOX=/path/to/firefox GECKODRIVER=/path/to/geckodriver python3 tests/test_firefox.py   (pip install selenium)"""
import os, sys, time, tempfile
sys.path.insert(0, os.path.dirname(__file__))
os.environ["UI_EDITABLE"] = "1"
from ui_common import Gui
from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.firefox.service import Service
from selenium.webdriver.support.ui import WebDriverWait

FF = os.environ.get("FIREFOX", "firefox"); GD = os.environ.get("GECKODRIVER", "geckodriver")
PASS = FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond: PASS += 1; print(f"  PASS  {name}")
    else: FAIL += 1; print(f"  FAIL  {name}  {str(detail)[:400]}")

d = tempfile.mkdtemp(); P = lambda n: os.path.join(d, n)
open(P("a.py"), "w").write("import os\ndef f(a):\n    return a\n")
open(P("t.csv"), "w").write("id,name,city\n" + "".join(f"{i},Name {i},{'Paris' if i%3==0 else 'Rome'}\n" for i in range(300)))
open(P("l.json"), "w").write('{"a":1,"b":[1,2]}\n'); open(P("l2.json"), "w").write('{"a":2,"b":[1,3]}\n')
open(P("n.md"), "w").write("# Title\n\n*it* `code`\n\n```py\ndef g(): return 1\n```\n")
open(P("e.json"), "w").write('{"a":{"b":[1,2,{"c":3}]},"s":"x"}\n')
open(P("pipe.txt"), "w").write("id|name\n1|Ann\n2|Bob\n3|Cy\n")
open(P("many.txt"), "w").write("".join(f"row {i} alpha beta gamma\n" for i in range(5000)))
try:
    import openpyxl
    wb = openpyxl.Workbook(); ws = wb.active; ws.title = "People"; ws.append(["name", "age"]); ws.append(["Ann", 31]); w2 = wb.create_sheet("Cities"); w2.append(["city"]); w2.append(["Paris"]); wb.save(P("b.xlsx")); X = True
except Exception: X = False

def driver(w=1300, h=800):
    o = Options(); o.add_argument("-headless"); o.binary_location = FF
    dr = webdriver.Firefox(options=o, service=Service(GD)); dr.set_window_size(w, h); return dr
def js(dr, code, *a): return dr.execute_script(code, *a)
def wait(dr, css, t=8, vis=True):
    from selenium.webdriver.support import expected_conditions as EC
    return WebDriverWait(dr, t).until(EC.visibility_of_element_located((By.CSS_SELECTOR, css)) if vis else EC.presence_of_element_located((By.CSS_SELECTOR, css)))
def keys(dr, *k): ActionChains(dr).send_keys(*k).perform()
def combo(dr, mod, k): ActionChains(dr).key_down(mod).send_keys(k).key_up(mod).perform()
def start(files, w=1300, h=800):
    g = Gui(files); dr = driver(w, h); dr.get(g.url); time.sleep(1.2)
    js(dr, "window.__errs = []; window.addEventListener('error', e => window.__errs.append(e.message));"); return g, dr
def toast(dr, text, t=6):
    end = time.time() + t
    while time.time() < end:
        if text in js(dr, "return (document.querySelector('#toasts')||document.body).innerText"): return True
        time.sleep(0.2)
    return False

print("Firefox", webdriver.Firefox.__name__)
print("== editor")
g, dr = start([P("a.py")])
check("Firefox is the browser", "Firefox" in js(dr, "return navigator.userAgent"), js(dr, "return navigator.userAgent"))
wait(dr, ".panel.show .ce-ta", vis=False)
check("file opens in the editor with colours", js(dr, "return document.querySelectorAll('.panel.show .ce-hl .sy-k, .panel.show .ce-hl .sy-c').length") >= 2)
dr.find_element(By.CSS_SELECTOR, ".panel.show .ce-ta").click(); combo(dr, Keys.CONTROL, Keys.END)
keys(dr, Keys.ENTER); keys(dr, "def nxt(a):"); keys(dr, Keys.ENTER); keys(dr, "pass")
v = js(dr, "return document.querySelector('.panel.show .ce-ta').value")
check("typing works and Enter after ':' indents", v.endswith("def nxt(a):\n  pass"), repr(v[-30:]))
combo(dr, Keys.CONTROL, "/"); v = js(dr, "return document.querySelector('.panel.show .ce-ta').value")
check("Ctrl+/ comments the line", v.endswith("# pass"), repr(v[-20:]))
combo(dr, Keys.CONTROL, "/")
combo(dr, Keys.CONTROL, "z"); combo(dr, Keys.CONTROL, "z"); combo(dr, Keys.CONTROL, "z")
check("Ctrl+Z undoes typing", "pass" not in js(dr, "return document.querySelector('.panel.show .ce-ta').value.split('\\n').pop()"))
combo(dr, Keys.CONTROL, "y"); combo(dr, Keys.CONTROL, "y"); combo(dr, Keys.CONTROL, "y")
check("tab shows the unsaved dot", "●" in js(dr, "return document.querySelector('.tab.active').innerText"))
combo(dr, Keys.CONTROL, "s"); check("Ctrl+S saves", toast(dr, "Saved"))
check("file on disk has the typed text", "def nxt(a):" in open(P("a.py")).read(), open(P("a.py")).read()[-40:])
combo(dr, Keys.CONTROL, Keys.HOME); ActionChains(dr).key_down(Keys.SHIFT).send_keys(Keys.ARROW_DOWN).send_keys(Keys.ARROW_DOWN).key_up(Keys.SHIFT).perform(); keys(dr, Keys.TAB); time.sleep(0.3)
check("Tab indents the selected lines", js(dr, "return document.querySelector('.panel.show .ce-ta').value.split('\\n')[1]").startswith("  def"))
dr.find_element(By.CSS_SELECTOR, ".panel.show .b-copy").click(); check("Copy button reports success", toast(dr, "Copied"))
check("no JS errors", js(dr, "return window.__errs.length") == 0, js(dr, "return window.__errs"))
dr.quit(); g.stop()

print("== find box")
g, dr = start([P("many.txt")])
wait(dr, ".panel.show .ce-ta", vis=False); dr.find_element(By.CSS_SELECTOR, ".panel.show .ce-ta").click(); combo(dr, Keys.CONTROL, "f"); wait(dr, ".panel.show .find .q")
js(dr, """window.__blank = 0; window.__w = new Set(); const f = document.querySelector('.panel.show .find'); const root = document.querySelector('.panel.show .editor');
  new MutationObserver(() => { if (root.querySelector('.line.loading')) window.__blank++; window.__w.add(Math.round(f.getBoundingClientRect().width)); }).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });""")
Q = lambda: dr.find_element(By.CSS_SELECTOR, ".panel.show .find .q")
for ch in "alpha beta -zzz": Q().send_keys(ch); time.sleep(0.12)
time.sleep(1.2)
check("no blank rows while typing in Find", js(dr, "return window.__blank") == 0, js(dr, "return window.__blank"))
check("find box keeps one width", js(dr, "return [...window.__w].length") <= 1, js(dr, "return [...window.__w]"))
check("matches counted", "line" in js(dr, "return document.querySelector('.panel.show .find .cnt').innerText"), js(dr, "return document.querySelector('.panel.show .find .cnt').innerText"))
check("three keyword chips", len(dr.find_elements(By.CSS_SELECTOR, ".panel.show .find .chip")) == 3)
Q().send_keys(Keys.ESCAPE); wait(dr, ".panel.show .ce-ta", vis=False)
check("Esc returns to the editor", js(dr, "return !document.querySelector('.panel.show .ce').hidden"))
dr.quit(); g.stop()

print("== table")
g, dr = start([P("t.csv")])
wait(dr, ".panel.show .tv .td", vis=False)
check("CSV opens as a table", js(dr, "return document.querySelectorAll('.panel.show .tv .hc').length") == 3)
js(dr, "document.querySelector('.panel.show .tv .hc').click()"); time.sleep(0.5)
check("header click sorts (no error)", js(dr, "return window.__errs.length") == 0)
dr.quit(); g.stop()
g, dr = start([P("pipe.txt")])
wait(dr, ".panel.show .tv .td", vis=False); time.sleep(1)
check("pipe-delimited .txt opens as a table", js(dr, "return document.querySelectorAll('.panel.show .tv .hc').length") == 2)
dr.quit(); g.stop()
if X:
    g, dr = start([P("b.xlsx")]); time.sleep(1.5)
    names = js(dr, "return [...document.querySelectorAll('.tab')].map(t => t.innerText)")
    check("Excel: one tab per sheet", len(names) == 2 and any("People" in n for n in names) and any("Cities" in n for n in names), names)
    dr.quit(); g.stop()

print("== compare")
g, dr = start([P("l.json"), P("l2.json")]); wait(dr, ".drow", vis=False)
check("compare opens", len(dr.find_elements(By.CSS_SELECTOR, ".drow")) >= 1)
dr.find_element(By.CSS_SELECTOR, ".drow .cell:not(.empty) .in").click(); wait(dr, ".cell-edit")
E = lambda: dr.find_element(By.CSS_SELECTOR, ".cell-edit"); js(dr, "const e=document.querySelector('.cell-edit'); e.select()"); E().send_keys('{"a":9}'); E().send_keys(Keys.ENTER); time.sleep(0.8)
check("click-to-edit a line in compare", '"a":9' in js(dr, "return document.querySelector('.panel.show .vsroot').innerText"))
dr.find_element(By.CSS_SELECTOR, ".panel.show .b-sl").click(); time.sleep(0.8)
check("Save L writes the edit", open(P("l.json")).read().startswith('{"a":9}'), open(P("l.json")).read())
dr.find_element(By.CSS_SELECTOR, ".panel.show .b-cr").click(); check("copy R button", toast(dr, "Copied the right"))
check("no JS errors", js(dr, "return window.__errs.length") == 0, js(dr, "return window.__errs"))
dr.quit(); g.stop()

print("== viewers")
g, dr = start([P("n.md")]); wait(dr, ".panel.show .ce-ta", vis=False)
dr.find_element(By.CSS_SELECTOR, ".panel.show .b-view").click(); wait(dr, "article.md pre.md-code")
check("Markdown preview with coloured code", js(dr, "return document.querySelectorAll('article.md pre.md-code [class^=sy-]').length") >= 1)
dr.quit(); g.stop()
g, dr = start([P("e.json")]); wait(dr, ".panel.show .ce-ta", vis=False)
dr.find_element(By.CSS_SELECTOR, ".panel.show .b-view").click(); time.sleep(0.8)
check("JSON tree renders", js(dr, "return document.querySelectorAll('.panel.show .jt-k').length") >= 2)
dr.find_element(By.CSS_SELECTOR, ".panel.show .b-fmt").click(); time.sleep(0.8)
check("no JS errors", js(dr, "return window.__errs.length") == 0, js(dr, "return window.__errs"))
dr.quit(); g.stop()

print("== clock and notes")
g, dr = start([P("a.py")])
dr.find_element(By.ID, "btn-clock").click(); wait(dr, ".wc .zrow")
check("clock shows two zones with 24 hour cells", len(dr.find_elements(By.CSS_SELECTOR, ".wc .zrow")) == 2 and len(dr.find_elements(By.CSS_SELECTOR, ".wc .zrow:first-child .strip span")) == 24)
s = dr.find_element(By.CSS_SELECTOR, ".wc .zrow:first-child .strip"); before = js(dr, "return document.querySelector('.wc .zrow .tm b').innerText")
ActionChains(dr).move_to_element_with_offset(s, -s.size["width"] // 2 + 15, 0).click().perform(); time.sleep(0.3)
check("clicking the left end of the strip sets the first hour of the day", js(dr, "return document.querySelector('.wc .zrow .tm b').innerText").startswith("00:"), js(dr, "return document.querySelector('.wc .zrow .tm b').innerText"))
check("marker is placed", js(dr, "return !document.querySelector('.wc .cur').hidden"))
dr.find_element(By.ID, "btn-notes").click(); wait(dr, ".nt .addrow")
t = dr.find_element(By.CSS_SELECTOR, ".nt .t"); t.send_keys("Firefox item", Keys.ENTER); time.sleep(0.4)
check("checklist adds an item", js(dr, "return [...document.querySelectorAll('.nt .it .tx')].map(e => e.value)") == ["Firefox item"])
dr.find_element(By.CSS_SELECTOR, ".nt .pad").send_keys("hello"); time.sleep(1.0)
check("no JS errors", js(dr, "return window.__errs.length") == 0, js(dr, "return window.__errs"))
dr.quit(); g.stop()

print("== dialogs, theme, layout")
g, dr = start([P("a.py")], 1000, 700)
dr.find_element(By.ID, "btn-new").click(); wait(dr, "#nf-name")
check("New file dialog has Browse and extension chips", js(dr, "return !!document.querySelector('#nf-browse') && document.querySelectorAll('.nf-ext').length >= 8"))
dr.find_element(By.CSS_SELECTOR, ".nf-ext[data-x=json]").click(); check("extension chip works", dr.find_element(By.ID, "nf-name").get_attribute("value").endswith(".json"))
keys(dr, Keys.ESCAPE); time.sleep(0.3)
dr.find_element(By.ID, "btn-open").click(); wait(dr, ".modal select.as")
check("Open dialog has 'Open as' choices", len(dr.find_elements(By.CSS_SELECTOR, ".modal select.as option")) >= 8)
keys(dr, Keys.ESCAPE); time.sleep(0.3)
dr.find_element(By.ID, "btn-settings").click(); wait(dr, ".modal")
check("Settings opens", True); keys(dr, Keys.ESCAPE); time.sleep(0.3)
m = js(dr, """const b = document.querySelector('.panel.show .toolbar'); const btns = [...b.querySelectorAll('button')].filter(x => x.offsetParent).map(x => x.getBoundingClientRect());
  return { over: b.scrollWidth > b.clientWidth + 1, rows: new Set(btns.map(r => Math.round(r.top))).size, sizes: [...new Set(btns.map(r => Math.round(r.width) + 'x' + Math.round(r.height)))], page: document.documentElement.scrollWidth <= document.documentElement.clientWidth };""")
check("toolbar: one row, no overflow, uniform buttons, page fits", not m["over"] and m["rows"] == 1 and m["sizes"] == ["34x28"] and m["page"], m)
dr.quit(); g.stop()
print(f"\n{PASS} passed, {FAIL} failed"); sys.exit(1 if FAIL else 0)
