"""Writes the fonts the ttf reader tests load: python3 make_fixtures.py

StaticLegacy.ttf: OS/2 version 1 (no sxHeight or sCapHeight), usWeightClass 500,
usWidthClass 6, fsType 0, a PANOSE, fsSelection REGULAR | USE_TYPO_METRICS;
head.fontRevision 2.005, lowestRecPPEM 9; hhea lineGap 25; style "Medium" (name
ID 17; ID 2 is "Regular"); a Japanese family name in a Windows and a MacJapanese
record; a kern table of two subtables (x H -30, H x -20, x x 5 in both,
x gravecomb -15). Glyphs:

* bar: hmtx lsb 110, glyf xMin 100.
* barflip: bar turned 180 degrees.
* bars: two bars; its own lsb is 5 units right of its xMin.
* barrot: bar turned 90 degrees (xy = -1, yx = 1).
* barx: x and bar, bar flagged USE_MY_METRICS; its own lsb is 40 units right of
  its xMin.
* gravecomb (U+0300, advance 0) and commaaccent (unencoded, advance 120).

GdefClasses.ttf: GDEF glyph classes (gravecomb a base with an advance, acutecomb a
mark without one, tildecomb a mark with one, f_f a ligature, script a base named
like a keyword); a calt rule whose five-glyph backtrack reads back as a named
class; a GPOS kern feature beside a kern table (a f_f -40); a mark feature; vhea
900/-100, every glyph 1000 high with its vertical origin at 600.

Unnamed.ttf: post format 3; A (U+0041), Aring (U+00C5, a component of A), an
unencoded alternate of A reached by salt, and A Aring kerned -50 in a kern table.

DuplicateNames.ttf: the post table names glyphs 2 and 3 both "dupe"; Acomp is a
component of glyph 3.

MarkNoGdef.ttf: no GDEF; a mark feature attaching gravecomb (U+0300) to a;
commaaccent unencoded; uni034F (U+034F) a default-ignorable nonspacing mark.
"""
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.ttLib.tables.O_S_2f_2 import Panose
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent
from fontTools.ttLib.tables._k_e_r_n import KernTable_format_0


def save(fb, path):
    # fixed dates, so that the same script writes the same bytes
    fb.font["head"].created = fb.font["head"].modified = 0
    fb.font.recalcTimestamp = False
    fb.save(path)


def legacy_kern(font, *subtables):
    font["kern"] = kern = newTable("kern")
    kern.version, kern.kernTables = 0, []
    for pairs in subtables:
        st = KernTable_format_0()
        st.version, st.coverage, st.kernTable = 0, 1, pairs
        kern.kernTables.append(st)


def rect(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()
    return pen.glyph()


def composite(*parts):
    g = Glyph()
    g.numberOfContours = -1
    g.components = []
    for name, dx, dy, matrix, *flags in parts:
        c = GlyphComponent()
        c.glyphName, c.x, c.y = name, dx, dy
        c.flags = 0x4 | (flags[0] if flags else 0)  # ROUND_XY_TO_GRID
        if matrix is not None:
            c.transform = matrix
        g.components.append(c)
    return g


USE_MY_METRICS = 0x200
order = [".notdef", "x", "H", "bar", "barflip", "bars", "barrot", "barx", "gravecomb",
         "commaaccent"]
glyphs = {
    ".notdef": Glyph(),
    "x": rect(50, 0, 450, 480),
    "H": rect(60, 0, 540, 700),
    "bar": rect(100, 0, 200, 600),
    "barflip": composite(("bar", 300, 600, [[-1, 0], [0, -1]])),
    "bars": composite(("bar", 0, 0, None), ("bar", 250, 0, None)),
    # fontTools' transform is [[xx, yx], [xy, yy]]: x' = xx*x + xy*y, y' = yx*x + yy*y
    "barrot": composite(("bar", 700, 0, [[0, 1], [-1, 0]])),
    "barx": composite(("x", 300, 0, None), ("bar", 0, 0, None, USE_MY_METRICS)),
    "gravecomb": rect(-200, 500, -100, 600),
    "commaaccent": rect(20, -200, 100, -50),
}
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({ord("x"): "x", ord("H"): "H", ord("|"): "bar", 0x0300: "gravecomb"})
fb.setupGlyf(glyphs)
lsb = {".notdef": 0, "x": 50, "H": 60, "bar": 110, "barflip": 100, "bars": 105, "barrot": 100, "barx": 140,
       "gravecomb": -200, "commaaccent": 20}
advance = {"gravecomb": 0, "commaaccent": 120}
fb.setupHorizontalMetrics({g: (advance.get(g, 800), lsb[g]) for g in order})
fb.setupHorizontalHeader(ascent=800, descent=-200, lineGap=25)
fb.setupNameTable({
    "familyName": "Fixture Medium",
    "styleName": "Regular",
    "typographicFamily": "Fixture",
    "typographicSubfamily": "Medium",
})
fb.setupOS2(
    version=1, usWeightClass=500, usWidthClass=6, fsType=0, fsSelection=0x40 | 0x80,
    sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200,
    panose=Panose(bFamilyType=2, bSerifStyle=0, bWeight=6, bProportion=3),
)
fb.setupPost()
fb.font["head"].fontRevision = 2.005
fb.font["head"].lowestRecPPEM = 9
# a Japanese family name, in a Windows record and a MacJapanese one
fb.font["name"].setName("\u3057\u3063\u307d\u308a", 1, 3, 1, 0x411)
fb.font["name"].setName("\u3057\u3063\u307d\u308a", 1, 1, 1, 11)
legacy_kern(fb.font, {("x", "H"): -30, ("x", "x"): 5, ("x", "gravecomb"): -15},
            {("H", "x"): -20, ("x", "x"): 5})
save(fb, "StaticLegacy.ttf")


order = [".notdef", "a", "gravecomb", "acutecomb", "tildecomb", "f_f", "script"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({ord("a"): "a", 0x0300: "gravecomb", 0x0301: "acutecomb", 0x0303: "tildecomb"})
fb.setupGlyf({".notdef": Glyph(), "a": rect(50, 0, 450, 500), "gravecomb": rect(40, 550, 150, 700),
              "acutecomb": rect(-150, 550, -40, 700), "tildecomb": rect(20, 550, 280, 650),
              "f_f": rect(50, 0, 750, 700), "script": rect(50, 0, 450, 500)})
advance = {".notdef": 500, "a": 500, "gravecomb": 190, "acutecomb": 0, "tildecomb": 300, "f_f": 800,
           "script": 500}
fb.setupHorizontalMetrics({g: (advance[g], fb.font["glyf"][g].xMin if g != ".notdef" else 0)
                           for g in order})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupVerticalHeader(ascent=900, descent=-100)
fb.setupVerticalMetrics({g: (1000, 600 - fb.font["glyf"][g].yMax if g != ".notdef" else 0)
                         for g in order})
fb.setupNameTable({"familyName": "Fixture Gdef", "styleName": "Regular"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
fb.setupPost()
addOpenTypeFeaturesFromString(fb.font, """
lookup up { sub gravecomb by acutecomb; } up;
feature calt {
    sub [a gravecomb acutecomb tildecomb f_f] gravecomb' lookup up;
} calt;
feature kern {
    pos a a -10;
} kern;
markClass acutecomb <anchor -100 600> @ACUTE;
feature mark {
    pos base a <anchor 250 500> mark @ACUTE;
} mark;
table GDEF {
    GlyphClassDef [a gravecomb \\script], [f_f], [acutecomb tildecomb], ;
} GDEF;
""")
legacy_kern(fb.font, {("a", "f_f"): -40})
save(fb, "GdefClasses.ttf")


order = [".notdef", "A", "Aring", "A.alt"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41: "A", 0xC5: "Aring"})
fb.setupGlyf({".notdef": Glyph(), "A": rect(50, 0, 550, 700), "Aring": composite(("A", 0, 0, None)),
              "A.alt": rect(60, 0, 560, 700)})
fb.setupHorizontalMetrics({".notdef": (500, 0), "A": (600, 50), "Aring": (600, 50), "A.alt": (600, 60)})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Fixture Unnamed", "styleName": "Regular"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
addOpenTypeFeaturesFromString(fb.font, "feature salt { sub A by A.alt; } salt;")
legacy_kern(fb.font, {("A", "Aring"): -50})
fb.setupPost(keepGlyphNames=False)
save(fb, "Unnamed.ttf")


order = [".notdef", "A", "dupa", "dupb", "Acomp"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41: "A", 0xC0: "Acomp"})
fb.setupGlyf({".notdef": Glyph(), "A": rect(50, 0, 550, 700), "dupa": rect(60, 0, 160, 100),
              "dupb": rect(70, 0, 170, 100), "Acomp": composite(("dupb", 0, 0, None))})
fb.setupHorizontalMetrics({".notdef": (500, 0), "A": (600, 50), "dupa": (600, 60), "dupb": (600, 70),
                           "Acomp": (600, 70)})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Fixture Duplicate Names", "styleName": "Regular"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
fb.setupPost()
save(fb, "DuplicateNames.ttf")
data = open("DuplicateNames.ttf", "rb").read()
# both names become "dupe": the same length, so nothing in the post table moves
assert data.count(b"\x04dupa") == 1 and data.count(b"\x04dupb") == 1
open("DuplicateNames.ttf", "wb").write(data.replace(b"\x04dupa", b"\x04dupe").replace(b"\x04dupb", b"\x04dupe"))


order = [".notdef", "a", "gravecomb", "commaaccent", "uni034F"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({ord("a"): "a", 0x0300: "gravecomb", 0x034F: "uni034F"})
fb.setupGlyf({".notdef": Glyph(), "a": rect(50, 0, 450, 500), "gravecomb": rect(-150, 550, -40, 700),
              "commaaccent": rect(20, -200, 100, -50), "uni034F": Glyph()})
fb.setupHorizontalMetrics({".notdef": (500, 0), "a": (500, 50), "gravecomb": (0, -150),
                           "commaaccent": (120, 20), "uni034F": (0, 0)})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Fixture Mark No Gdef", "styleName": "Regular"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
fb.setupPost()
addOpenTypeFeaturesFromString(fb.font, """
markClass gravecomb <anchor -100 600> @TOP;
feature mark {
    pos base a <anchor 250 500> mark @TOP;
} mark;
""")
del fb.font["GDEF"]
save(fb, "MarkNoGdef.ttf")
