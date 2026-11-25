"""Builds anchordefault.ttf: a top anchor centred on A at the default
weight, far left at the lightest. Run: python3 anchordefault.py"""
from fontTools.fontBuilder import FontBuilder
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.pens.ttGlyphPen import TTGlyphPen


def box(x0, x1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, 0))
    pen.lineTo((x0, 700))
    pen.lineTo((x1, 700))
    pen.lineTo((x1, 0))
    pen.closePath()
    return pen.glyph()


order = [".notdef", "A", "acutecomb"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x41: "A", 0x301: "acutecomb"})
fb.setupGlyf({".notdef": box(50, 450), "A": box(0, 600), "acutecomb": box(-150, -50)})
fb.setupHorizontalMetrics({".notdef": (500, 50), "A": (600, 0), "acutecomb": (0, -150)})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Anchor Default", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupFvar([("wght", 100, 400, 900, "Weight")], [])
addOpenTypeFeaturesFromString(
    fb.font,
    """table GDEF { GlyphClassDef [A], , [acutecomb], ; } GDEF;
languagesystem DFLT dflt;
markClass acutecomb <anchor -100 700> @M;
feature mark {
    pos base A <anchor (wght=100:60 wght=400:300 wght=900:320) 700> mark @M;
} mark;""",
)
fb.save("anchordefault.ttf")
