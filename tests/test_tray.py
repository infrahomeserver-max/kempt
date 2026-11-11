"""Linux tray icon against a simulated StatusNotifierWatcher on a private session bus.
Run:  pip install dbus-next && dbus-run-session -- python3 tests/test_tray.py"""
import asyncio, os, sys, subprocess, tempfile, time, json
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface, method, dbus_property, signal
from dbus_next import Variant, BusType
from dbus_next.constants import PropertyAccess

BIN = os.environ.get("KEMPT_BIN") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "kempt")
registered = []
class Watcher(ServiceInterface):
    def __init__(self): super().__init__("org.kde.StatusNotifierWatcher")
    @method()
    def RegisterStatusNotifierItem(self, service: 's'): registered.append(service)
    @method()
    def RegisterStatusNotifierHost(self, service: 's'): pass
    @dbus_property(access=PropertyAccess.READ)
    def RegisteredStatusNotifierItems(self) -> 'as': return registered
    @dbus_property(access=PropertyAccess.READ)
    def IsStatusNotifierHostRegistered(self) -> 'b': return True
    @dbus_property(access=PropertyAccess.READ)
    def ProtocolVersion(self) -> 'i': return 0
    @signal()
    def StatusNotifierItemRegistered(self) -> 's': return ""

async def main():
    bus = await MessageBus(bus_type=BusType.SESSION).connect()
    bus.export("/StatusNotifierWatcher", Watcher())
    await bus.request_name("org.kde.StatusNotifierWatcher")
    d = tempfile.mkdtemp(); log = os.path.join(d, "open.log")
    os.makedirs(d + "/bin"); open(d + "/bin/xdg-open", "w").write(f'#!/bin/sh\necho "$@" >> {log}\n'); os.chmod(d + "/bin/xdg-open", 0o755)
    env = dict(os.environ, KEMPT_CONFIG_DIR=d + "/cfg", PATH=d + "/bin:" + os.environ["PATH"])
    env.pop("KEMPT_NO_TRAY", None)
    p = subprocess.Popen([BIN, "gui", "--background"], env=env)
    for _ in range(50):
        if registered: break
        await asyncio.sleep(0.2)
    print("registered:", registered)
    ok = bool(registered)
    if ok:
        svc = registered[0]; name = svc.split("/")[0]; path = "/" + svc.split("/", 1)[1] if "/" in svc else "/StatusNotifierItem"
        intro = await bus.introspect(name, path); obj = bus.get_proxy_object(name, path, intro)
        props = obj.get_interface("org.freedesktop.DBus.Properties")
        title = (await props.call_get("org.kde.StatusNotifierItem", "Title")).value
        pix = (await props.call_get("org.kde.StatusNotifierItem", "IconPixmap")).value
        print("title:", title, "pixmap:", [(w, h, len(b)) for w, h, b in pix])
        ok &= title == "Kempt" and pix and pix[0][0] == 32 and len(pix[0][2]) == 32 * 32 * 4
        sni = obj.get_interface("org.kde.StatusNotifierItem"); await sni.call_activate(0, 0)
        await asyncio.sleep(1.0)
        print("left click opened:", open(log).read().strip() if os.path.exists(log) else None)
        ok &= os.path.exists(log) and "http://127.0.0.1:" in open(log).read()
        menu_path = (await props.call_get("org.kde.StatusNotifierItem", "Menu")).value
        mi = await bus.introspect(name, menu_path); mo = bus.get_proxy_object(name, menu_path, mi)
        dm = mo.get_interface("com.canonical.dbusmenu")
        rev, layout = await dm.call_get_layout(0, -1, [])
        labels = [c.value[1].get("label", Variant("s", "—")).value for c in layout[2]]
        print("menu:", labels)
        ok &= labels == ["Open Kempt", "Checklist & reminders", "World clock", "—", "Quit Kempt"]
        ids = [c.value[0] for c in layout[2]]
        await dm.call_event(ids[1], "clicked", Variant("i", 0), 0)
        await asyncio.sleep(1.0)
        last = open(log).read().strip().splitlines()[-1]
        print("menu item opened:", last); ok &= last.endswith("&view=notes")
        try: await dm.call_event(ids[4], "clicked", Variant("i", 0), 0)
        except Exception as e: print("(no reply, process exited as expected)")
        for _ in range(30):
            if p.poll() is not None: break
            await asyncio.sleep(0.2)
        print("quit exited:", p.poll()); ok &= p.poll() == 0 and not os.path.exists(d + "/cfg/instance.json")
    p.kill() if p.poll() is None else None
    print("TRAY", "OK" if ok else "FAILED"); sys.exit(0 if ok else 1)
asyncio.run(main())
