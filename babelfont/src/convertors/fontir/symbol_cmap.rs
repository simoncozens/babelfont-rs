use write_fonts::{
    from_obj::ToOwnedTable,
    read::{FontRef, TableProvider},
    tables::cmap::{Cmap, PlatformId},
    FontBuilder,
};

use crate::{BabelfontError, Font};

/// The key a font states its Windows character map is symbol-encoded under
/// (platform 3, encoding 0), as symbol fonts' are: Webdings, Wingdings and
/// Hoefler Text Ornaments map their glyphs from U+F020 up there. A compiler
/// writes a Unicode one (3, 1), which applications do not read as symbols.
pub(crate) const KEY_SYMBOL_ENCODING: &str = "com.counterpunch.cmap.symbolEncoding";

/// Whether the font states its Windows character map is symbol-encoded
pub(crate) fn is_symbol_encoded(font: &Font) -> bool {
    font.format_specific
        .get(KEY_SYMBOL_ENCODING)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Re-encodes a compiled binary's Windows BMP character map as symbol (3, 0)
pub(crate) fn encode_as_symbol(binary: &[u8]) -> Result<Vec<u8>, BabelfontError> {
    let fontref = FontRef::new(binary)?;
    let mut cmap: Cmap = fontref.cmap()?.to_owned_table();
    for record in cmap.encoding_records.iter_mut() {
        if record.platform_id == PlatformId::Windows && record.encoding_id == 1 {
            record.encoding_id = 0;
        }
    }
    cmap.encoding_records
        .sort_by_key(|record| (record.platform_id as u16, record.encoding_id));
    let mut builder = FontBuilder::new();
    builder
        .add_table(&cmap)
        .map_err(|e| BabelfontError::General(format!("Error adding cmap table: {e:#?}")))?;
    builder.copy_missing_tables(fontref);
    Ok(builder.build())
}
