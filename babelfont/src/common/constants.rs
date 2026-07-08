/// A mapping of OS/2 weight values to their corresponding names
pub const OS2_WEIGHT_TO_NAME_MAP: &[(u16, &str)] = &[
    (100, "Thin"),
    (200, "ExtraLight"),
    (200, "UltraLight"), // Alias for ExtraLight; most common goes first
    (300, "Light"),
    (400, "Regular"),
    (400, "Normal"),
    (500, "Medium"),
    (600, "SemiBold"),
    (600, "DemiBold"),
    (700, "Bold"),
    (800, "ExtraBold"),
    (800, "UltraBold"),
    (900, "Black"),
];

/// A mapping of OS/2 width values to their corresponding names
pub const OS2_WIDTH_TO_NAME_MAP: &[(u16, &str)] = &[
    (1, "UltraCondensed"),
    (2, "ExtraCondensed"),
    (3, "Condensed"),
    (4, "SemiCondensed"),
    (5, "Medium"),
    (5, "Normal"),
    (6, "SemiExpanded"),
    (7, "Expanded"),
    (8, "ExtraExpanded"),
    (9, "UltraExpanded"),
];
