"""Builds a small .xlsb (BIFF12) workbook by hand, so the reader can be tested without Excel.
Usage: make_xlsb.py OUT.xlsb
Sheet1: text, numbers (RK int, RK x100, real), a date, bool, error, blank gap row, shared + inline strings.
Sheet2 (hidden sheet 3 is skipped)."""
import struct, sys, zipfile

def rec(i, payload=b""):
    out = bytes([i]) if i < 0x80 else bytes([(i & 0x7F) | 0x80, i >> 7])   # ids are 7 bits per byte, low part first
    n = len(payload); lb = b""
    while True:
        c = n & 0x7F; n >>= 7
        lb += bytes([c | (0x80 if n else 0)])
        if not n: break
    return out + lb + payload

def ws(s):  # XLWideString
    return struct.pack("<I", len(s)) + s.encode("utf-16-le")

def cell(i, col, style, data): return rec(i, struct.pack("<II", col, style) + data)

def sheet(rows):
    b = rec(0x81) + rec(148, struct.pack("<IIII", 0, 5, 0, 3)) + rec(0x91)   # BeginSheet, WsDim, SheetData
    for r, cells in rows:
        b += rec(0, struct.pack("<I", r) + b"\0" * 21)
        for c in cells: b += c
    return b + rec(0x92) + rec(0x82)

SS = ["Name", "Café ☕", "total"]
strings = b"".join(rec(19, b"\0" + ws(s)) for s in SS)
strings = rec(0x9F, struct.pack("<II", len(SS), len(SS))) + strings + rec(0xA0)

# styles: xf0 general, xf1 = builtin date 14, xf2 = custom date fmt 164 (yyyy-mm-dd hh:mm)
def xf(fmt): return rec(47, struct.pack("<HH", 0, fmt) + b"\0" * 8)
styles = (rec(615, struct.pack("<I", 1)) + rec(44, struct.pack("<H", 164) + ws("yyyy\\-mm\\-dd\\ hh:mm")) + rec(616)
          + rec(626, struct.pack("<I", 1)) + xf(0) + rec(627)
          + rec(617, struct.pack("<I", 3)) + xf(0) + xf(14) + xf(164) + rec(618))

rk_int = lambda n: struct.pack("<I", (n << 2) | 2)
rk_x100 = lambda n: struct.pack("<I", (n << 2) | 2 | 1)
s1 = sheet([
    (0, [cell(7, 0, 0, struct.pack("<I", 0)), cell(7, 1, 0, struct.pack("<I", 1)), cell(7, 2, 0, struct.pack("<I", 2))]),
    (1, [cell(6, 0, 0, ws("inline, with comma")), cell(2, 1, 0, rk_int(42)), cell(2, 2, 0, rk_x100(1234))]),
    (2, [cell(5, 0, 0, struct.pack("<d", 3.14159)), cell(5, 1, 1, struct.pack("<d", 45000.0)), cell(5, 2, 2, struct.pack("<d", 44197.5))]),
    (4, [cell(4, 0, 0, b"\1"), cell(3, 1, 0, b"\x07"), cell(1, 2, 0, b""), cell(9, 3, 0, struct.pack("<d", 7.0) + b"\0\0")]),
])
s2 = sheet([(0, [cell(7, 0, 0, struct.pack("<I", 0))])])
def bundle(state, tab, rid, name): return rec(156, struct.pack("<II", state, tab) + ws(rid) + ws(name))
wb = rec(0x83) + rec(153, struct.pack("<I", 0) + b"\0" * 8) + rec(0x8F) + bundle(0, 1, "rId1", "Data") + bundle(0, 2, "rId2", "Other") + bundle(2, 3, "rId3", "Secret") + rec(0x90) + rec(0x84)
rels = '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + "".join(
    f'<Relationship Id="rId{i}" Type="x" Target="worksheets/sheet{i}.bin"/>' for i in (1, 2, 3)) + "</Relationships>"
ct = '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="bin" ContentType="application/vnd.ms-excel.sheet.binary.macroEnabled.main"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.bin" ContentType="application/vnd.ms-excel.sheet.binary.macroEnabled.main"/></Types>'
root = '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.bin"/></Relationships>'
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", ct); z.writestr("_rels/.rels", root)
    z.writestr("xl/workbook.bin", wb); z.writestr("xl/_rels/workbook.bin.rels", rels)
    z.writestr("xl/sharedStrings.bin", strings); z.writestr("xl/styles.bin", styles)
    z.writestr("xl/worksheets/sheet1.bin", s1); z.writestr("xl/worksheets/sheet2.bin", s2); z.writestr("xl/worksheets/sheet3.bin", s2)
