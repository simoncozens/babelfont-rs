"""Builds lookupregions.ttf: a pair lookup whose values vary over two
regions, one of them only over the second. Run: python3 lookupregions.py"""
from fontTools.fontBuilder import FontBuilder
from fontTools.otlLib.builder import buildValue
from fontTools.ttLib.tables import otTables as ot
from fontTools.varLib.builder import buildVarRegionList, buildVarData, buildVarStore
from fontTools.pens.ttGlyphPen import TTGlyphPen

order = [".notdef", "a", "b", "c"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x61: "a", 0x62: "b", 0x63: "c"})
pen = TTGlyphPen(None)
fb.setupGlyf({g: pen.glyph() for g in order})
fb.setupHorizontalMetrics({g: (500, 0) for g in order})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Lookup Regions", "styleName": "Regular"})
fb.setupOS2()
fb.setupPost()
fb.setupFvar([("wght", 100, 100, 900, "Weight")], [])
# region 0 peaks at wght 500, region 1 at 900; a c varies over region 1 only
regions = buildVarRegionList([{"wght": (0, 0.5, 1)}, {"wght": (0, 1, 1)}], ["wght"])
data = buildVarData([0, 1], [[-10, -20], [0, -40]], optimize=False)
store = buildVarStore(regions, [data])
gdef = ot.GDEF()
gdef.Version = 0x00010003
gdef.GlyphClassDef = None
gdef.AttachList = None
gdef.LigCaretList = None
gdef.MarkAttachClassDef = None
gdef.MarkGlyphSetsDef = None
gdef.VarStore = store
from fontTools.ttLib import newTable
fb.font["GDEF"] = table = newTable("GDEF")
table.table = gdef


def value(index, inner):
    v = buildValue({"XAdvance": index})
    dev = ot.Device()
    dev.DeltaFormat = 0x8000
    dev.StartSize = 0
    dev.EndSize = inner
    v.XAdvDevice = dev
    return v


from fontTools.otlLib.builder import buildPairPosGlyphsSubtable, buildLookup
pairs = {("a", "b"): (value(-30, 0), None), ("a", "c"): (value(-50, 1), None)}
subtable = buildPairPosGlyphsSubtable(pairs, fb.font.getReverseGlyphMap())
lookup = buildLookup([subtable])
gpos = ot.GPOS()
gpos.Version = 0x00010000
gpos.LookupList = ot.LookupList()
gpos.LookupList.Lookup = [lookup]
gpos.LookupList.LookupCount = 1
feature = ot.Feature()
feature.FeatureParams = None
feature.LookupListIndex = [0]
feature.LookupCount = 1
record = ot.FeatureRecord()
record.FeatureTag = "kern"
record.Feature = feature
gpos.FeatureList = ot.FeatureList()
gpos.FeatureList.FeatureRecord = [record]
gpos.FeatureList.FeatureCount = 1
langsys = ot.LangSys()
langsys.LookupOrder = None
langsys.ReqFeatureIndex = 0xFFFF
langsys.FeatureIndex = [0]
langsys.FeatureCount = 1
script = ot.Script()
script.DefaultLangSys = langsys
script.LangSysRecord = []
script.LangSysCount = 0
srec = ot.ScriptRecord()
srec.ScriptTag = "DFLT"
srec.Script = script
gpos.ScriptList = ot.ScriptList()
gpos.ScriptList.ScriptRecord = [srec]
gpos.ScriptList.ScriptCount = 1
gpos.FeatureVariations = None
from fontTools.ttLib import newTable
fb.font["GPOS"] = t = newTable("GPOS")
t.table = gpos
fb.save("lookupregions.ttf")
